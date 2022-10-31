use std::os::unix::io::AsRawFd;
use std::time::Duration;

use log::info;
use nix::errno::Errno;
use nix::sys::epoll::EpollFlags;
use nix::sys::time::TimeSpec;
use nix::sys::timer::TimerSetTimeFlags;
use nix::sys::timerfd::{ClockId, Expiration, TimerFd, TimerFlags};

use learning_async::epoll::{Epoll, FdType};

fn main() -> color_eyre::Result<()> {
    aqa_logger::init();
    info!("Hello world");

    let mut epoll = Epoll::new()?;

    let timers = [
        (
            TimerFd::new(ClockId::CLOCK_REALTIME, TimerFlags::empty())?,
            Duration::from_secs(5),
        ),
        (
            TimerFd::new(ClockId::CLOCK_REALTIME, TimerFlags::empty())?,
            Duration::from_secs(1),
        ),
        (
            TimerFd::new(ClockId::CLOCK_REALTIME, TimerFlags::empty())?,
            Duration::from_secs(3),
        ),
        (
            TimerFd::new(ClockId::CLOCK_REALTIME, TimerFlags::empty())?,
            Duration::from_millis(100),
        ),
        (
            TimerFd::new(ClockId::CLOCK_REALTIME, TimerFlags::empty())?,
            Duration::from_secs(1),
        ),
        (
            TimerFd::new(ClockId::CLOCK_REALTIME, TimerFlags::empty())?,
            Duration::from_secs(2),
        ),
        (
            TimerFd::new(ClockId::CLOCK_REALTIME, TimerFlags::empty())?,
            Duration::from_secs(3),
        ),
        (
            TimerFd::new(ClockId::CLOCK_REALTIME, TimerFlags::empty())?,
            Duration::from_secs(4),
        ),
    ];
    timers.iter().try_for_each(|(timerfd, duration)| {
        epoll.add_fd(timerfd.as_raw_fd(), EpollFlags::EPOLLIN, FdType::Timer)?;
        timerfd.set(
            Expiration::OneShot(TimeSpec::from_duration(*duration)),
            TimerSetTimeFlags::empty(),
        )?;
        Result::<_, Errno>::Ok(())
    })?;

    epoll.start();

    Ok(())
}
