use log::debug;
use std::future::Future;
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::time::TimeSpec;
use nix::sys::timer::{Expiration, TimerSetTimeFlags};
use nix::sys::timerfd::{ClockId, TimerFd, TimerFlags};

use crate::runtime::{RuntimeMsg, RuntimeMsgQueue};

pub struct Sleep {
    timer: TimerFd,
    duration: Duration,
    started: Option<Instant>,
}

pub fn sleep(duration: Duration) -> Sleep {
    Sleep::new(duration).unwrap()
}

impl Sleep {
    pub fn new(duration: Duration) -> Result<Self, Errno> {
        let timerfd = TimerFd::new(
            ClockId::CLOCK_REALTIME,
            TimerFlags::TFD_CLOEXEC | TimerFlags::TFD_NONBLOCK,
        )?;

        Ok(Sleep {
            timer: timerfd,
            duration,
            started: None,
        })
    }

    fn expired(&self) -> bool {
        self.started
            .map(|started| started + self.duration <= Instant::now())
            .unwrap_or_default()
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.started.is_none() {
            debug!("Sleep not started. Registering.");
            self.started = Some(Instant::now());
            self.timer
                .set(
                    Expiration::OneShot(TimeSpec::from_duration(self.duration)),
                    TimerSetTimeFlags::empty(),
                )
                .expect("failed to set timerfd");

            RuntimeMsgQueue::try_current()
                .expect("no runtime")
                .send(RuntimeMsg::RegisterTimer {
                    waker: cx.waker().clone(),
                    timer_fd: self.timer.as_raw_fd(),
                });

            Poll::Pending
        } else if self.expired() {
            Poll::Ready(())
        } else {
            debug!("Timer still pending");
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use log::debug;
    use std::time::{Duration, Instant};

    use crate::runtime::Runtime;
    use crate::sleep::sleep;

    #[test]
    fn test_sleep() -> color_eyre::Result<()> {
        aqa_logger::init();

        let mut runtime = Runtime::new()?;

        let start = Instant::now();
        runtime.block_on(async move {
            sleep(Duration::from_millis(20)).await;
        });
        let elapsed = start.elapsed();
        debug!("elapsed: {}", humantime::format_duration(elapsed));
        assert!(elapsed > Duration::from_millis(20));
        assert!(elapsed < Duration::from_millis(21));

        Ok(())
    }
}
