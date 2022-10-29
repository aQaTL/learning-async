use std::collections::HashMap;
use std::future::Future;
use std::ops::DerefMut;
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use log::{debug, error, info};
use nix::sys::epoll::EpollEvent;
use nix::unistd::read;
use thiserror::Error;

use crate::epoll::{Epoll, EventData, FdType};
use crate::signal::Signal;

static WAKER_VTABLE: RawWakerVTable =
    RawWakerVTable::new(clone_fn, wake_fn, wake_by_ref_fn, drop_fn);

pub struct Runtime {
    waker_signal_fd: Arc<Signal>,

    epoll: Epoll,

    next_future_id: u64,
    futures: HashMap<u64, (Box<dyn Future<Output = ()>>, Waker)>,
}

struct WakerContext {
    _runtime_handle: Weak<Runtime>,
    future_id: u64,
    waker_signal_fd: Arc<Signal>,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("epoll error: {0:?}")]
    Epoll(nix::errno::Errno),
}

impl Runtime {
    pub fn new() -> Result<Self, RuntimeError> {
        let mut epoll = Epoll::new().map_err(RuntimeError::Epoll)?;
        let waker_signal_fd = Arc::new(epoll.add_signal().map_err(RuntimeError::Epoll)?);

        let rt = Runtime {
            waker_signal_fd,
            epoll,
            next_future_id: 143,
            futures: HashMap::default(),
        };

        Ok(rt)
    }

    pub fn spawn<F>(&mut self, fut: F)
    where
        F: Future<Output = ()> + 'static,
    {
        let future_id = self.next_future_id;
        self.next_future_id += 1;

        let waker_ctx_ptr = Arc::into_raw(Arc::new(WakerContext {
            _runtime_handle: Default::default(),
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

        match fut.poll(&mut cx) {
            Poll::Ready(()) => {
                self.futures.remove(&future_id);
            }
            Poll::Pending => {
                // self.epoll.add_signal(future_id).unwrap();
            }
        }
    }

    pub fn block_on<F>(&mut self, fut: F) -> <F as Future>::Output
    where
        F: Future<Output = ()> + 'static,
    {
        self.spawn(fut);

        let mut events_buf = vec![EpollEvent::empty(); 128];
        let mut buf = [0_u8; 1024];

        loop {
            // for mut fut in &mut self.futures {
            //     let fut = unsafe { Pin::new_unchecked(&mut fut) };
            //     if let Poll::Ready(v) = fut.poll(&mut cx) {
            //         return v;
            //     }
            // }

            for event in self.epoll.poll(&mut events_buf).unwrap() {
                let event_data = EventData { u64: event.data() };
                let fd = unsafe { event_data.fd };
                let kind = self.epoll.registered.get(&fd).unwrap();
                debug!("Received event on fd {fd}");

                let bytes = match read(fd, &mut buf) {
                    Ok(bytes_read) if bytes_read == 0 => {
                        self.epoll.remove_fd(fd).unwrap();
                        self.epoll.registered.remove(&fd);
                        continue;
                    }
                    Ok(bytes_read) => &buf[0..bytes_read],
                    Err(err) => {
                        error!("Read failed. {err:?}");
                        continue;
                    }
                };
                debug!("Bytes read: {}", bytes.len());

                match kind {
                    FdType::EventFd => {
                        let val = u64::from_ne_bytes(bytes[..8].try_into().unwrap());
                        if fd != self.waker_signal_fd.as_raw_fd() {
                            info!("Received signal({val}) from eventfd {fd}");
                        } else {
                            let future_id = val;
                            let (fut, waker) = match self.futures.get_mut(&future_id) {
                                Some(v) => v,
                                None => {
                                    error!("Recieved signal for unknown future (ID {val})");
                                    continue;
                                }
                            };

                            let mut cx = Context::from_waker(waker);

                            // let fut: Pin<&mut Box<dyn Future<Output = () >>> = fut.as_mut();
                            let fut: Pin<&mut (dyn Future<Output = ()> + 'static)> =
                                unsafe { Pin::new_unchecked(fut.deref_mut()) };

                            match fut.poll(&mut cx) {
                                Poll::Ready(()) => {
                                    self.futures.remove(&future_id);
                                }
                                Poll::Pending => {}
                            }
                        }
                    }
                    FdType::Terminal | FdType::Socket => {
                        let msg = String::from_utf8_lossy(bytes);

                        info!("Received message: \"{msg}\"");
                        if fd == 0 && msg.trim() == "stop" {
                            return;
                        }
                    }
                }
            }
        }
    }
}

unsafe fn clone_fn(waker_ctx: *const ()) -> RawWaker {
    debug!("clone_fn");

    let waker_ctx: Arc<WakerContext> = Arc::from_raw(waker_ctx.cast::<WakerContext>());
    let new_waker_ctx = Arc::clone(&waker_ctx);

    //Prevent deallocating pointer passed into this fn
    let _ = Arc::into_raw(waker_ctx);
    RawWaker::new(Arc::into_raw(new_waker_ctx).cast::<()>(), &WAKER_VTABLE)
}

unsafe fn wake_fn(waker_ctx: *const ()) {
    debug!("wake_fn");

    wake_by_ref_fn(waker_ctx);

    let _: Arc<WakerContext> = Arc::from_raw(waker_ctx.cast::<WakerContext>());
}

unsafe fn wake_by_ref_fn(waker_ctx: *const ()) {
    debug!("wake_by_ref_fn");

    let waker_ctx: Arc<WakerContext> = Arc::from_raw(waker_ctx.cast::<WakerContext>());
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

pub struct TcpConnection {}
