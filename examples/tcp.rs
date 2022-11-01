use learning_async::runtime::Runtime;
use learning_async::tcp::TcpStream;
use log::info;

fn main() -> color_eyre::Result<()> {
    aqa_logger::init();

    let mut rt = Runtime::new()?;

    rt.block_on(async move {
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

        info!("Received message: {}", String::from_utf8_lossy(&main_buf));
        info!("Message size: {}", main_buf.len());
    });

    Ok(())
}
