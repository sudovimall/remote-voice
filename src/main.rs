pub mod config;

use tracing_subscriber::EnvFilter;
use crate::config::settings::init_config;

type R<T> = anyhow::Result<T>;

#[tokio::main]
async fn main() -> R<()> {
    init_log();
    let config = init_config()?;

    Ok(())
}

/// log
fn init_log() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_file(true)
        .with_target(true)
        .with_ansi(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .init()
}
