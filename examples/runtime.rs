use log::info;

use learning_async::runtime::Runtime;

fn main() -> color_eyre::Result<()> {
    aqa_logger::init();

    let mut runtime = Runtime::new()?;
    runtime.block_on(async move {
        info!("Hello world form async function");
    });

    Ok(())
}
