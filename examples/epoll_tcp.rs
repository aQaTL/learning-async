use std::net::TcpStream;
use std::os::unix::io::AsRawFd;

use log::info;
use nix::sys::epoll::EpollFlags;

use learning_async::epoll::{example_epoll_event_loop, Epoll, FdType};

fn main() -> color_eyre::Result<()> {
    aqa_logger::init();
    info!("Hello world");

    let mut epoll = Epoll::new()?;

    let tcp_stream = TcpStream::connect("127.0.0.1:8000")?;
    epoll.add_fd(tcp_stream.as_raw_fd(), EpollFlags::EPOLLIN, FdType::Socket)?;

    example_epoll_event_loop(epoll);

    Ok(())
}
