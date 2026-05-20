use tracing_subscriber::EnvFilter;
use voice::{Result, app, config::settings::init_config};

#[tokio::main]
async fn main() -> Result<()> {
    init_log();
    let config = init_config()?;
    app::serve(config).await
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
