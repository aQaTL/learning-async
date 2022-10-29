use log::{debug, error};
use nix::unistd::{close, write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};

pub struct Signal {
    fd: RawFd,
}

impl Signal {
    pub fn signal(&self, signal_val: u64) -> Result<(), std::io::Error> {
        let v = write(self.fd, &signal_val.to_ne_bytes())?;
        debug!("Signal({v}) sent on fd {}", self.fd);
        Ok(())
    }
}

impl FromRawFd for Signal {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Signal { fd }
    }
}

impl AsRawFd for Signal {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        if let Err(err) = close(self.fd) {
            error!("Failed to close Signal fd: {err:?}");
        }
    }
}
