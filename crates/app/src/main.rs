//! nus entry point. Nothing here yet beyond logging; the window comes in
//! once spikes 1–4 (docs/SPIKES.md) have landed their results.

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    tracing::info!("nus {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
