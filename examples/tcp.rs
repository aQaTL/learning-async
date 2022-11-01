use std::time::{Duration, Instant};

use log::info;

use learning_async::runtime::Runtime;
use learning_async::sleep::sleep;
use learning_async::tcp::TcpStream;

fn main() -> color_eyre::Result<()> {
    aqa_logger::init();

    let mut rt = Runtime::new()?;

    rt.spawn(async move {
        let start = Instant::now();
        info!("2. Connecting");
        let mut tcp_stream = TcpStream::connect("127.0.0.1:8001").await.unwrap();

        //TODO send something

        info!("2. Reading response");
        let mut main_buf = Vec::new();
        let mut buf = vec![0_u8; 1024];
        loop {
            let read_bytes = tcp_stream.read(&mut buf).await.unwrap();
            info!("2. Received {read_bytes} bytes",);
            main_buf.extend_from_slice(&buf[..read_bytes]);
            if read_bytes == 0 {
                break;
            }
        }

        // info!("2. Received message: {}", String::from_utf8_lossy(&main_buf));
        info!("2. Message size: {}", main_buf.len());
        let elapsed = start.elapsed();
        info!(
            "Secondary fut elapsed: {}",
            humantime::format_duration(elapsed)
        );
    });

    let start = Instant::now();

    rt.block_on(async move {
        let start = Instant::now();
        info!("Connecting");
        let mut tcp_stream = TcpStream::connect("127.0.0.1:8000").await.unwrap();

        //TODO send something

        info!("Reading response");
        let mut main_buf = Vec::new();
        let mut buf = vec![0_u8; 1024];
        loop {
            let read_bytes = tcp_stream.read(&mut buf).await.unwrap();
            info!("Received {read_bytes} bytes",);
            main_buf.extend_from_slice(&buf[..read_bytes]);
            if read_bytes == 0 {
                break;
            }
        }

        // info!("Received message: {}", String::from_utf8_lossy(&main_buf));
        info!("Message size: {}", main_buf.len());
        let elapsed = start.elapsed();
        info!("Main fut elapsed: {}", humantime::format_duration(elapsed));
        sleep(Duration::from_millis(2)).await;
        let elapsed = start.elapsed();
        info!("After sleep : {}", humantime::format_duration(elapsed));
    });

    let elapsed = start.elapsed();
    info!("Total elapsed: {}", humantime::format_duration(elapsed));

    Ok(())
}
