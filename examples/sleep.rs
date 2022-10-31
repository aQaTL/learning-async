use std::time::{Duration, Instant};

use log::info;

use learning_async::runtime::Runtime;
use learning_async::sleep::sleep;

fn main() -> color_eyre::Result<()> {
    aqa_logger::init();

    let mut runtime = Runtime::new()?;

    runtime.spawn(async move {
        info!("Hello world form spawned async function 1");
        let now = Instant::now();
        sleep(Duration::from_secs(1)).await;
        let elapsed = now.elapsed();
        info!(
            "Slept for 1 second, but actually for {}",
            humantime::format_duration(elapsed)
        );
    });

    runtime.spawn(async move {
        info!("Hello world form spawned async function 2");
        let now = Instant::now();
        sleep(Duration::from_secs(3)).await;
        let elapsed = now.elapsed();
        info!(
            "Slept for 3 seconds, but actually for {}",
            humantime::format_duration(elapsed)
        );
    });

    runtime.spawn(async move {
        info!("Hello world form spawned async function 3");
        let now = Instant::now();
        sleep(Duration::from_millis(300)).await;
        let elapsed = now.elapsed();
        info!(
            "Slept for 300 milliseconds, but actually for {}",
            humantime::format_duration(elapsed)
        );
    });

    runtime.block_on(async move {
        info!("Hello world form main async function");
        let now = Instant::now();
        sleep(Duration::from_secs(5)).await;
        let elapsed = now.elapsed();
        info!(
            "Slept for 5 seconds, but actually for {}",
            humantime::format_duration(elapsed)
        );
    });

    Ok(())
}
