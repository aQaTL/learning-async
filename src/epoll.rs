use std::collections::HashMap;
use std::os::unix::io::{FromRawFd, RawFd};
use std::time::Duration;

use log::{debug, error, info};
use nix::errno::Errno;
use nix::sys::epoll::{
    epoll_create1, epoll_ctl, epoll_wait, EpollCreateFlags, EpollEvent, EpollFlags, EpollOp,
};
use nix::sys::eventfd::{eventfd, EfdFlags};
use nix::unistd::{close, read};

use crate::signal::Signal;

pub struct Epoll {
    fd: RawFd,
    timeout: Duration,

    pub(crate) registered: HashMap<RawFd, FdType>,
}

#[derive(Copy, Clone)]
pub enum FdType {
    Socket,
    Terminal,
    EventFd,
}

impl Epoll {
    pub fn new() -> Result<Self, Errno> {
        let fd = epoll_create1(EpollCreateFlags::empty())?;
        Ok(Epoll {
            fd,
            timeout: Duration::from_secs(30),
            registered: HashMap::default(),
        })
    }

    pub fn add_fd(&mut self, fd: RawFd, flags: EpollFlags, kind: FdType) -> Result<(), Errno> {
        debug!("Registering fd {fd}");
        let event_data = EventData { fd };
        let mut event = EpollEvent::new(flags, unsafe { event_data.u64 });

        epoll_ctl(self.fd, EpollOp::EpollCtlAdd, fd, &mut event)?;

        self.registered.insert(fd, kind);

        Ok(())
    }

    pub fn remove_fd(&self, fd: RawFd) -> Result<(), Errno> {
        info!("Removing fd {fd}");
        let event_data = EventData { fd };
        let mut event = EpollEvent::new(EpollFlags::empty(), unsafe { event_data.u64 });
        epoll_ctl(self.fd, EpollOp::EpollCtlDel, fd, &mut event)
    }

    pub fn add_signal(&mut self) -> Result<Signal, Errno> {
        let init_value = 0;
        let fd = eventfd(init_value, EfdFlags::EFD_NONBLOCK | EfdFlags::EFD_CLOEXEC)?;
        self.add_fd(fd, EpollFlags::EPOLLIN, FdType::EventFd)?;

        Ok(unsafe { Signal::from_raw_fd(fd) })
    }

    pub fn start(mut self) {
        let mut events_buf = vec![EpollEvent::empty(); 128];
        let mut buf = [0_u8; 1024];

        loop {
            debug!("Polling for input...");

            let event_count = epoll_wait(
                self.fd,
                &mut events_buf,
                self.timeout.as_millis().try_into().unwrap(),
            )
            .unwrap();
            debug!("Polled {event_count} events");

            for event in &events_buf[0..event_count] {
                let data = EventData { u64: event.data() };
                let fd = unsafe { data.fd };
                let kind = self.registered.get(&fd).unwrap();

                debug!("Reading file descriptor {fd}");

                let bytes = match read(fd, &mut buf) {
                    Ok(bytes_read) if bytes_read == 0 => {
                        self.remove_fd(fd).unwrap();
                        self.registered.remove(&fd);
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
                        info!("Received signal({val}) from eventfd {fd}");
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

    pub fn poll<'a, 'b>(
        &'a mut self,
        events: &'b mut [EpollEvent],
    ) -> Result<&'b [EpollEvent], Errno> {
        debug!("Polling...");
        let event_count = epoll_wait(
            self.fd,
            events,
            self.timeout.as_millis().try_into().unwrap(),
        )?;
        debug!("Polled {event_count} events");

        Ok(&events[..event_count])
    }
}

impl Drop for Epoll {
    fn drop(&mut self) {
        if let Err(err) = close(self.fd) {
            error!("Failed to close Epoll fd: {err:?}");
        }
    }
}

#[repr(C)]
pub(crate) union EventData {
    pub ptr: *mut std::ffi::c_void,
    pub fd: i32,
    pub u32: u32,
    pub u64: u64,
}
