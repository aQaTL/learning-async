use log::info;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use learning_async::runtime::Runtime;

fn main() -> color_eyre::Result<()> {
    aqa_logger::init();

    let mut runtime = Runtime::new()?;

    runtime.spawn(async move {
        info!("Hello world form spawned async function 1");
        sleep(Duration::from_secs(1)).await;
        info!("Slept for 1 second");
    });

    runtime.spawn(async move {
        info!("Hello world form spawned async function 2");
        sleep(Duration::from_secs(3)).await;
        info!("Slept for 3 seconds");
    });

    runtime.spawn(async move {
        info!("Hello world form spawned async function 3");
        sleep(Duration::from_millis(300)).await;
        info!("Slept for 300 milliseconds");
    });

    runtime.block_on(async move {
        info!("Hello world form main async function");
        sleep(Duration::from_secs(5)).await;
        info!("Slept for 5 seconds");
    });

    Ok(())
}

fn sleep(duration: Duration) -> Sleep {
    Sleep::new(duration)
}

struct Sleep {
    duration: Duration,
    started_at: Option<Instant>,
}

impl Sleep {
    fn new(duration: Duration) -> Self {
        Self {
            duration,
            started_at: None,
        }
    }

    fn expired(&self) -> bool {
        self.started_at
            .map(|started_at| Instant::now() >= (started_at + self.duration))
            .unwrap_or_default()
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.expired() {
            Poll::Ready(())
        } else {
            if self.started_at.is_none() {
                self.started_at = Some(Instant::now());
            }

            let waker = cx.waker().clone();
            let duration = self.duration;
            std::thread::spawn(move || {
                std::thread::sleep(duration);
                waker.wake();
            });

            Poll::Pending
        }
    }
}
