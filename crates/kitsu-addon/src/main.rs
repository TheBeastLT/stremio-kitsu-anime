use kitsu_addon::infrastructure::{Config, logging};

fn main() {
    logging::init();
    let config = Config::from_env();
    tracing::info!("Configured addon on port {}", config.port);
}
