/* standard use */
use std::time::Instant;

use panacus::run_cli;

fn main() -> Result<(), anyhow::Error> {
    env_logger::init();
    let timer = Instant::now();

    // print output to stdout
    run_cli()?;

    let duration = timer.elapsed();
    log::info!("done; time elapsed: {:?} ", duration);

    Ok(())
}
