use log::info;
use nix::sys::epoll::EpollFlags;
use std::net::TcpListener;
use std::os::unix::io::AsRawFd;

use learning_async::epoll::{example_epoll_event_loop, Epoll, FdType};

fn main() -> color_eyre::Result<()> {
    aqa_logger::init();
    info!("Hello world");

    let mut epoll = Epoll::new()?;
    epoll.add_fd(
        std::io::stdin().as_raw_fd(),
        EpollFlags::EPOLLIN,
        FdType::Terminal,
    )?;

    let tcp_listener = TcpListener::bind("localhost:8000")?;

    let (tcp_stream, addr) = tcp_listener.accept()?;
    info!(
        "Received connection from {addr} at fd {}",
        tcp_stream.as_raw_fd()
    );
    epoll.add_fd(tcp_stream.as_raw_fd(), EpollFlags::EPOLLIN, FdType::Socket)?;

    example_epoll_event_loop(epoll);

    Ok(())
}
