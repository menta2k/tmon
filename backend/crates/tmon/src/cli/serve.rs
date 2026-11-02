//! `tmon serve`.

use std::path::Path;

use crate::config::Config;

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,tower_http=warn".into());
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

async fn wait_for_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(s) => s,
                Err(_) => return ctrl_c.await.unwrap_or_default(),
            };
        tokio::select! {
            _ = ctrl_c => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    ctrl_c.await.unwrap_or_default();
}

pub async fn run(config: Option<&Path>) -> anyhow::Result<()> {
    init_tracing();
    let validated = Config::load(config)?.validate()?;
    for w in &validated.warnings {
        tracing::warn!("{w}");
    }
    let running = crate::app::start(validated).await?;
    wait_for_signal().await;
    tracing::info!("shutting down; flushing storage");
    running.shutdown().await;
    Ok(())
}
