use std::collections::HashMap;
use std::future::Future;
use std::ops::{ControlFlow, DerefMut};
use std::os::unix::io::{AsRawFd, RawFd};
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use log::{debug, error, info, warn};
use nix::errno::Errno;
use nix::sys::epoll::{EpollEvent, EpollFlags};
use nix::unistd::read;
use thiserror::Error;

use crate::epoll::{Epoll, EventData, FdType, MyEventData};
use crate::signal::Signal;

static WAKER_VTABLE: RawWakerVTable =
    RawWakerVTable::new(clone_fn, wake_fn, wake_by_ref_fn, drop_fn);

static mut RUNTIME_MSG_QUEUE: Option<Arc<RuntimeMsgQueue>> = None;

pub struct Runtime {
    msg_queue: Arc<RuntimeMsgQueue>,
    // Moved into a separate struct to separate msg_queue borrow from the rest of [Runtime] data
    event_loop: RuntimeEventLoopData,
}

struct RuntimeEventLoopData {
    waker_signal_fd: Arc<Signal>,

    epoll: Epoll,

    next_future_id: u64,
    main_future_id: u64,

    futures: HashMap<u64, (Box<dyn Future<Output = ()>>, Waker)>,
    timers: HashMap<RawFd, Waker>,
    network_sockets: HashMap<RawFd, Waker>,
}

struct WakerContext {
    future_id: u64,
    waker_signal_fd: Arc<Signal>,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("epoll error: {0:?}")]
    Epoll(Errno),
}

impl Runtime {
    pub fn new() -> Result<Self, RuntimeError> {
        let mut epoll = Epoll::new().map_err(RuntimeError::Epoll)?;
        let waker_signal_fd = Arc::new(epoll.add_signal().map_err(RuntimeError::Epoll)?);

        let rt = Runtime {
            event_loop: RuntimeEventLoopData {
                waker_signal_fd,
                epoll,
                next_future_id: 143,
                main_future_id: 142,
                futures: HashMap::default(),
                timers: HashMap::default(),
                network_sockets: HashMap::default(),
            },
            msg_queue: Default::default(),
        };

        unsafe {
            RUNTIME_MSG_QUEUE = Some(Arc::clone(&rt.msg_queue));
        }

        Ok(rt)
    }

    pub fn spawn<F>(&mut self, fut: F)
    where
        F: Future<Output = ()> + 'static,
    {
        self.event_loop.spawn(fut)
    }

    pub fn block_on<F>(&mut self, fut: F) -> <F as Future>::Output
    where
        F: Future<Output = ()> + 'static,
    {
        self.event_loop.main_future_id = self.event_loop.next_future_id;
        self.spawn(fut);

        // If the main future completed after the first poll, return
        if self
            .event_loop
            .futures
            .get(&self.event_loop.main_future_id)
            .is_none()
        {
            return;
        }

        let mut events_buf = vec![EpollEvent::empty(); 128];

        let mut futures_to_poll = Vec::new();

        loop {
            let rt = &mut self.event_loop;

            while !self.msg_queue.queue.read().unwrap().is_empty() {
                debug!("Looping over msg_queue");
                for msg in self.msg_queue.queue.write().unwrap().drain(..) {
                    match msg {
                        RuntimeMsg::Wake { future_id } => futures_to_poll.push(future_id),
                        RuntimeMsg::RegisterTimer { waker, timer_fd } => {
                            if let Err(err) =
                                rt.epoll
                                    .add_fd(timer_fd, EpollFlags::EPOLLIN, FdType::Timer)
                            {
                                error!("Failed to register timer: {err:?} ({})", err.desc());
                            }
                            rt.timers.insert(timer_fd, waker);
                            debug!("Timer {timer_fd} registered");
                        }
                        RuntimeMsg::RegisterTcpStream { waker, socket } => {
                            match rt.epoll.add_fd(
                                socket,
                                // EpollFlags::EPOLLIN | EpollFlags::EPOLLOUT,
                                EpollFlags::EPOLLIN,
                                FdType::Socket,
                            ) {
                                Err(Errno::EEXIST) | Ok(()) => (),
                                Err(err) => {
                                    error!(
                                        "Failed to register tcp stream: {err:?} ({})",
                                        err.desc()
                                    );
                                }
                            }
                            rt.network_sockets.insert(socket, waker);
                            debug!("TcpStream {socket} registered");
                        }
                    }
                }

                debug!("Polling {} futures", futures_to_poll.len());
                for future_id in futures_to_poll.drain(..) {
                    match rt.poll_fut(future_id) {
                        ControlFlow::Break(()) => return,
                        ControlFlow::Continue(()) => continue,
                    }
                }
            }

            for event in rt.epoll.poll(&mut events_buf).unwrap() {
                let MyEventData { fd, kind } = unsafe { EventData { u64: event.data() }.data };
                debug!(
                    "Received events ({events:?}) on fd {fd} with registered type {kind:?}",
                    events = event.events()
                );

                let result: LoopFlow = match kind {
                    FdType::EventFd => rt.handle_event_on_eventfd(fd),
                    FdType::Socket => rt.handle_event_on_socket(fd),
                    FdType::Terminal => rt.handle_event_on_terminal(fd),
                    FdType::Timer => rt.handle_event_on_timer(fd),
                };
                match result {
                    ControlFlow::Continue(()) => continue,
                    ControlFlow::Break(()) => return,
                }
            }
            events_buf
                .iter_mut()
                .for_each(|event| *event = EpollEvent::empty());
        }
    }
}

type LoopFlow = ControlFlow<()>;

impl RuntimeEventLoopData {
    pub fn handle_event_on_eventfd(&mut self, fd: i32) -> LoopFlow {
        let mut buf = [0_u8; 8];
        let bytes = match read(fd, &mut buf) {
            Ok(bytes_read) if bytes_read == 0 => {
                self.epoll.remove_fd(fd).unwrap();
                return LoopFlow::Continue(());
            }
            Ok(bytes_read) => &buf[0..bytes_read],
            Err(err) => {
                error!("Read failed. {err:?}");
                return LoopFlow::Continue(());
            }
        };
        debug!("Bytes read: {}", bytes.len());
        debug_assert!(bytes.len() >= 8, "expected to read 8 bytes");
        let val = u64::from_ne_bytes(bytes[..8].try_into().unwrap());

        if fd != self.waker_signal_fd.as_raw_fd() {
            warn!("Received signal({val}) from eventfd {fd}");
            ControlFlow::Continue(())
        } else {
            self.poll_fut(val)
        }
    }

    pub fn handle_event_on_socket(&mut self, fd: i32) -> LoopFlow {
        match self.network_sockets.get(&fd) {
            Some(waker) => waker.wake_by_ref(),
            None => panic!("No future for TcpStream {fd}"),
        }
        LoopFlow::Continue(())
    }

    pub fn handle_event_on_terminal(&mut self, fd: i32) -> LoopFlow {
        let mut buf = [0_u8; 1024];
        let bytes = match read(fd, &mut buf) {
            Ok(bytes_read) if bytes_read == 0 => {
                self.epoll.remove_fd(fd).unwrap();
                return LoopFlow::Continue(());
            }
            Ok(bytes_read) => &buf[0..bytes_read],
            Err(err) => {
                error!("Read failed. {err:?}");
                return LoopFlow::Continue(());
            }
        };
        debug!("Bytes read: {}", bytes.len());

        let msg = String::from_utf8_lossy(bytes);

        info!("Received message: \"{msg}\"");
        if fd == 0 && msg.trim() == "stop" {
            return LoopFlow::Break(());
        }
        LoopFlow::Continue(())
    }

    pub fn handle_event_on_timer(&mut self, fd: i32) -> LoopFlow {
        debug!("Timer {fd} fired!");
        let waker = match self.timers.remove(&fd) {
            Some(v) => v,
            None => {
                error!("No future for timer {fd}");
                return LoopFlow::Continue(());
            }
        };

        if let Err(err) = self.epoll.remove_fd(fd) {
            error!("Failed to remove timerfd fd from epoll. {err:?}");
        }

        waker.wake();

        LoopFlow::Continue(())
    }

    pub fn spawn<F>(&mut self, fut: F)
    where
        F: Future<Output = ()> + 'static,
    {
        let future_id = self.next_future_id;
        self.next_future_id += 1;

        let waker_ctx_ptr = Arc::into_raw(Arc::new(WakerContext {
            future_id,
            waker_signal_fd: Arc::clone(&self.waker_signal_fd),
        }));
        let waker =
            unsafe { Waker::from_raw(RawWaker::new(waker_ctx_ptr.cast::<()>(), &WAKER_VTABLE)) };

        self.futures.insert(future_id, (Box::new(fut), waker));

        let (fut, waker) = self.futures.get_mut(&future_id).unwrap();
        let mut cx = Context::from_waker(waker);

        // let fut: Pin<&mut Box<dyn Future<Output = () >>> = fut.as_mut();
        let fut: Pin<&mut (dyn Future<Output = ()> + 'static)> =
            unsafe { Pin::new_unchecked(fut.deref_mut()) };

        if let Poll::Ready(()) = fut.poll(&mut cx) {
            self.futures.remove(&future_id);
        }
    }

    fn poll_fut(&mut self, future_id: u64) -> ControlFlow<()> {
        let (fut, waker) = match self.futures.get_mut(&future_id) {
            Some(v) => v,
            None => {
                error!("Received signal for unknown future (ID {future_id})");
                return ControlFlow::Continue(());
            }
        };

        let mut cx = Context::from_waker(waker);

        let fut: Pin<&mut (dyn Future<Output = ()> + 'static)> =
            unsafe { Pin::new_unchecked(fut.deref_mut()) };

        match fut.poll(&mut cx) {
            Poll::Ready(()) => {
                debug!("Removing future {future_id}");
                self.futures.remove(&future_id);
                if future_id == self.main_future_id {
                    return ControlFlow::Break(());
                }
            }
            Poll::Pending => (),
        }

        ControlFlow::Continue(())
    }
}

unsafe fn clone_fn(waker_ctx: *const ()) -> RawWaker {
    debug!("clone_fn");

    let waker_ctx: Arc<WakerContext> = Arc::from_raw(waker_ctx.cast::<WakerContext>());
    let new_waker_ctx = Arc::clone(&waker_ctx);

    //Prevent deallocating pointer passed into this fn
    let _ = Arc::into_raw(waker_ctx);
    let data = Arc::into_raw(new_waker_ctx).cast::<()>();
    RawWaker::new(data, &WAKER_VTABLE)
}

unsafe fn wake_fn(waker_ctx: *const ()) {
    debug!("wake_fn");

    wake_by_ref_fn(waker_ctx);

    let _: Arc<WakerContext> = Arc::from_raw(waker_ctx.cast::<WakerContext>());
}

unsafe fn wake_by_ref_fn(waker_ctx: *const ()) {
    debug!("wake_by_ref_fn");

    let waker_ctx: Arc<WakerContext> = Arc::from_raw(waker_ctx.cast::<WakerContext>());

    // Using msg queue here doesn't work, because when all no future uses epoll (but for example
    // self spawned threads), epoll will block until timeout before we can process this message.
    // RuntimeMsgQueue::try_current()
    //     .unwrap()
    //     .send(RuntimeMsg::Wake {
    //         future_id: waker_ctx.future_id,
    //     });

    if let Err(err) = waker_ctx.waker_signal_fd.signal(waker_ctx.future_id) {
        error!("Failed to signal wake. {err:?}");
    }

    //Prevent deallocating pointer passed into this fn
    let _ = Arc::into_raw(waker_ctx);
}

unsafe fn drop_fn(waker_ctx: *const ()) {
    debug!("drop_fn");
    drop(Arc::from_raw((waker_ctx as *mut ()).cast::<WakerContext>()))
}

#[derive(Default)]
pub(crate) struct RuntimeMsgQueue {
    queue: RwLock<Vec<RuntimeMsg>>,
}

impl RuntimeMsgQueue {
    pub(crate) fn try_current() -> Option<&'static RuntimeMsgQueue> {
        unsafe { RUNTIME_MSG_QUEUE.as_deref() }
    }

    pub(crate) fn current() -> &'static RuntimeMsgQueue {
        RuntimeMsgQueue::try_current().expect("no runtime")
    }

    pub fn send(&self, msg: RuntimeMsg) {
        self.queue
            .write()
            .expect("Failed to acquire RuntimeMessageQueue lock")
            .push(msg)
    }
}

pub(crate) enum RuntimeMsg {
    RegisterTimer {
        waker: Waker,
        timer_fd: RawFd,
    },
    RegisterTcpStream {
        waker: Waker,
        socket: RawFd,
    },
    #[allow(dead_code)]
    Wake {
        future_id: u64,
    },
}
