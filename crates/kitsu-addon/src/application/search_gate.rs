use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use tokio::sync::Semaphore;

use super::Error;

pub struct SearchGate {
    permits: Arc<Semaphore>,
    waiting: AtomicUsize,
    queue: usize,
}

struct Waiting<'a>(&'a AtomicUsize);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl SearchGate {
    pub fn new(concurrency: usize, queue: usize) -> Self {
        Self { permits: Arc::new(Semaphore::new(concurrency)), waiting: AtomicUsize::new(0), queue }
    }

    pub fn available(&self) -> usize {
        self.permits.available_permits()
    }

    pub fn waiting(&self) -> usize {
        self.waiting.load(Ordering::SeqCst)
    }

    pub async fn run<T: Send + 'static>(&self, work: impl FnOnce() -> T + Send + 'static) -> Result<T, Error> {
        let permit = if let Ok(permit) = Arc::clone(&self.permits).try_acquire_owned() {
            permit
        } else {
            if self.waiting.fetch_add(1, Ordering::SeqCst) >= self.queue {
                self.waiting.fetch_sub(1, Ordering::SeqCst);
                return Err(Error::Busy("search queue full".to_owned()));
            }
            let _waiting = Waiting(&self.waiting);
            Arc::clone(&self.permits)
                .acquire_owned()
                .await
                .map_err(|error| Error::failed(anyhow::anyhow!("search gate closed: {error}")))?
        };
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .map_err(|error| Error::failed(anyhow::anyhow!("search failed: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Condvar, Mutex},
        time::Duration,
    };

    use super::*;

    #[derive(Default)]
    struct Latch {
        open: Mutex<bool>,
        changed: Condvar,
    }

    impl Latch {
        fn wait(&self) {
            let mut open = self.open.lock().unwrap();
            while !*open {
                open = self.changed.wait(open).unwrap();
            }
        }

        fn release(&self) {
            *self.open.lock().unwrap() = true;
            self.changed.notify_all();
        }
    }

    struct Released(Arc<Latch>);

    impl Drop for Released {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    async fn until(condition: impl Fn() -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(tokio::time::Instant::now() < deadline, "condition not reached");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    fn blocked(gate: &Arc<SearchGate>, latch: &Arc<Latch>) -> tokio::task::JoinHandle<Result<(), Error>> {
        let (gate, latch) = (Arc::clone(gate), Arc::clone(latch));
        tokio::spawn(async move { gate.run(move || latch.wait()).await })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn at_most_the_configured_searches_run_at_once() {
        let gate = Arc::new(SearchGate::new(2, 32));
        let latch = Arc::new(Latch::default());
        let _released = Released(Arc::clone(&latch));
        let tasks: Vec<_> = (0..3).map(|_| blocked(&gate, &latch)).collect();
        until(|| gate.waiting() == 1).await;
        assert_eq!(gate.available(), 0);
        latch.release();
        for task in tasks {
            task.await.unwrap().unwrap();
        }
        assert_eq!((gate.available(), gate.waiting()), (2, 0));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_search_beyond_the_queue_is_busy() {
        let gate = Arc::new(SearchGate::new(1, 1));
        let latch = Arc::new(Latch::default());
        let _released = Released(Arc::clone(&latch));
        let running = blocked(&gate, &latch);
        let queued = blocked(&gate, &latch);
        until(|| gate.waiting() == 1).await;
        let overflow = gate.run(|| ()).await;
        assert!(matches!(overflow, Err(Error::Busy(_))));
        latch.release();
        running.await.unwrap().unwrap();
        queued.await.unwrap().unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_abandoned_search_keeps_its_permit_until_it_ends() {
        let gate = Arc::new(SearchGate::new(1, 4));
        let latch = Arc::new(Latch::default());
        let _released = Released(Arc::clone(&latch));
        let abandoned = blocked(&gate, &latch);
        until(|| gate.available() == 0).await;
        abandoned.abort();
        assert!(abandoned.await.unwrap_err().is_cancelled());
        assert_eq!(gate.available(), 0);
        latch.release();
        until(|| gate.available() == 1).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_abandoned_waiter_leaves_the_queue() {
        let gate = Arc::new(SearchGate::new(1, 1));
        let latch = Arc::new(Latch::default());
        let _released = Released(Arc::clone(&latch));
        let running = blocked(&gate, &latch);
        until(|| gate.available() == 0).await;
        let waiter = blocked(&gate, &latch);
        until(|| gate.waiting() == 1).await;
        waiter.abort();
        let _ = waiter.await;
        assert_eq!(gate.waiting(), 0);
        latch.release();
        running.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn a_panicking_search_is_a_failure_and_frees_its_permit() {
        let gate = SearchGate::new(1, 1);
        let result: Result<(), Error> = gate.run(|| panic!("index broken")).await;
        assert!(matches!(result, Err(Error::Failed(_))));
        assert_eq!(gate.available(), 1);
    }
}
