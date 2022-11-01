use std::time::Duration;

use log::info;

use learning_async::epoll::{example_epoll_event_loop, Epoll};

fn main() -> color_eyre::Result<()> {
    aqa_logger::init();
    info!("Hello world");

    let mut epoll = Epoll::new()?;

    let signal = epoll.add_signal()?;

    std::thread::spawn(move || {
        let signals = [634, 1239, 9, 413, 58129];
        for signal_val in &signals {
            std::thread::sleep(Duration::from_secs(1));
            signal.signal(*signal_val).unwrap();
        }
        std::thread::sleep(Duration::from_secs(1));
    });

    example_epoll_event_loop(epoll);

    Ok(())
}
