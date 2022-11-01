use std::future::Future;
use std::net::{SocketAddr, ToSocketAddrs};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::pin::Pin;
use std::task::{Context, Poll};

use log::debug;
use nix::errno::Errno;
use nix::sys::socket::{
    connect, recv, socket, AddressFamily, MsgFlags, SockFlag, SockType, SockaddrStorage,
};

use crate::runtime::{RuntimeMsg, RuntimeMsgQueue};

pub struct TcpStream {
    stream: std::net::TcpStream,
}

impl TcpStream {
    pub fn connect(addr: impl ToSocketAddrs) -> impl Future<Output = Result<TcpStream, Errno>> {
        let addr: SocketAddr = addr.to_socket_addrs().unwrap().next().unwrap();
        TcpStreamConnect { addr, socket: None }
    }

    pub fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Read<'a, Self> {
        Read {
            reader: self,
            buf,
            bytes_read: 0,
        }
    }
}

impl AsRawFd for TcpStream {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

struct TcpStreamConnect {
    addr: SocketAddr,
    socket: Option<RawFd>,
}

impl Future for TcpStreamConnect {
    type Output = Result<TcpStream, Errno>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        debug!("Polling TcpStreamConnect");
        let socket = match self.socket {
            Some(v) => {
                debug!("Socket already created");
                v
            }
            None => {
                let address_family = match self.addr {
                    SocketAddr::V4(..) => AddressFamily::Inet,
                    SocketAddr::V6(..) => AddressFamily::Inet6,
                };
                match socket(
                    address_family,
                    SockType::Stream,
                    SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
                    None,
                ) {
                    Ok(socket) => {
                        self.socket = Some(socket);
                        debug!("Created socket {socket}");
                        socket
                    }
                    Err(err) => return Poll::Ready(Err(err)),
                }
            }
        };

        let socketaddr_storage: SockaddrStorage = self.addr.into();
        match connect(socket, &socketaddr_storage) {
            Ok(()) => (),
            Err(Errno::EINPROGRESS) => {
                debug!("Connection in progress");
                let waker = cx.waker().clone();
                RuntimeMsgQueue::current().send(RuntimeMsg::RegisterTcpStream { socket, waker });

                return Poll::Pending;
            }
            Err(err) => return Poll::Ready(Err(err)),
        }

        let stream = unsafe { std::net::TcpStream::from_raw_fd(socket) };
        Poll::Ready(Ok(TcpStream { stream }))
    }
}

pub trait AsyncRead {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, Errno>>;
}

pub struct Read<'a, T> {
    reader: &'a mut T,
    buf: &'a mut [u8],
    bytes_read: usize,
}

impl<'a, T> Future for Read<'a, T>
where
    T: AsRawFd,
{
    type Output = Result<usize, Errno>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            debug!(
                "Polling read on fd {}. Our buffer is {}. Bytes read {}",
                self.reader.as_raw_fd(),
                self.buf.len(),
                self.bytes_read
            );
            let bytes_read = self.bytes_read;
            let result = recv(
                self.reader.as_raw_fd(),
                &mut self.buf[bytes_read..],
                MsgFlags::empty(),
            );
            debug!("poll result {result:?}");
            match result {
                Ok(0) => return Poll::Ready(Ok(self.bytes_read)),
                Err(Errno::EAGAIN) => {
                    RuntimeMsgQueue::current().send(RuntimeMsg::RegisterTcpStream {
                        waker: cx.waker().clone(),
                        socket: self.reader.as_raw_fd(),
                    });
                    return Poll::Pending;
                }
                Ok(bytes_read) => {
                    self.bytes_read += bytes_read;
                    if self.bytes_read >= self.buf.len() {
                        return Poll::Ready(Ok(self.bytes_read));
                    }
                    continue;
                }
                Err(err) => return Poll::Ready(Err(err)),
            }
        }
    }
}
