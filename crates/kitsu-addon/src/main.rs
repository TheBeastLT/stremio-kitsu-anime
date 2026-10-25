use kitsu_addon::{
    application::REFRESH_INTERVAL,
    bootstrap,
    infrastructure::{Config, logging},
    shutdown::Signals,
};

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

const DEFAULT_BLOCKING_THREADS: usize = 8;

fn blocking_threads(configured: Option<String>) -> usize {
    configured.and_then(|value| value.parse().ok()).filter(|threads| *threads > 0).unwrap_or(DEFAULT_BLOCKING_THREADS)
}

fn main() -> anyhow::Result<()> {
    let threads = blocking_threads(std::env::var("TOKIO_MAX_BLOCKING_THREADS").ok());
    tokio::runtime::Builder::new_multi_thread().enable_all().max_blocking_threads(threads).build()?.block_on(run())
}

async fn run() -> anyhow::Result<()> {
    logging::init();
    let signals = Signals::install()?;
    let config = Config::from_env();
    let service = bootstrap::build(&config).await?;
    let listener = bootstrap::bind(config.port).await?;
    tracing::info!("Started addon at: http://localhost:{}", listener.local_addr()?.port());
    let refresh = service.refresh.spawn(REFRESH_INTERVAL);
    bootstrap::serve(listener, service.state, signals.graceful()).await?;
    refresh.abort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocking_thread_cap_defaults_when_unset_or_invalid() {
        assert_eq!(blocking_threads(None), DEFAULT_BLOCKING_THREADS);
        assert_eq!(blocking_threads(Some("abc".to_owned())), DEFAULT_BLOCKING_THREADS);
        assert_eq!(blocking_threads(Some("0".to_owned())), DEFAULT_BLOCKING_THREADS);
        assert_eq!(blocking_threads(Some("16".to_owned())), DEFAULT_BLOCKING_THREADS * 2);
    }
}
