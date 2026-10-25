use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    Ok,
    Error,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
        }
    }
}

pub trait UpstreamObserver: Send + Sync {
    fn observe(&self, upstream: &'static str, outcome: Outcome, duration: Duration);
}

pub struct NoUpstreamStats;

impl UpstreamObserver for NoUpstreamStats {
    fn observe(&self, _upstream: &'static str, _outcome: Outcome, _duration: Duration) {}
}
