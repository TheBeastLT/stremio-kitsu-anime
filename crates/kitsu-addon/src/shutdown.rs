use std::io;

use tokio::signal::unix::{Signal, SignalKind, signal};

pub struct Signals {
    terminate: Signal,
    interrupt: Signal,
}

impl Signals {
    pub fn install() -> io::Result<Self> {
        Ok(Self { terminate: signal(SignalKind::terminate())?, interrupt: signal(SignalKind::interrupt())? })
    }

    async fn next(&mut self) -> &'static str {
        tokio::select! {
            _ = self.terminate.recv() => "SIGTERM",
            _ = self.interrupt.recv() => "SIGINT",
        }
    }

    pub async fn graceful(mut self) {
        let name = self.next().await;
        tracing::info!("{name} received, shutting down");
        tokio::spawn(async move {
            let name = self.next().await;
            tracing::info!("{name} received again, exiting");
            std::process::exit(0);
        });
    }
}
