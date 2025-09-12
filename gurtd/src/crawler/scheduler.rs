use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore, OwnedSemaphorePermit};

#[derive(Clone)]
pub struct HostScheduler {
    global: Arc<Semaphore>,
    per_host_limit: usize,
    hosts: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
    // Politeness gate per host to honor crawl-delay when requested
    polite: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<Option<Instant>>>>>>,
}

impl HostScheduler {
    pub fn new(global_limit: usize, per_host_limit: usize) -> Self {
        Self {
            global: Arc::new(Semaphore::new(global_limit)),
            per_host_limit,
            hosts: Arc::new(Mutex::new(HashMap::new())),
            polite: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn host_sem(&self, host: &str) -> Arc<Semaphore> {
        let mut map = self.hosts.lock().await;
        if let Some(s) = map.get(host) { return s.clone(); }
        let s = Arc::new(Semaphore::new(self.per_host_limit));
        map.insert(host.to_string(), s.clone());
        s
    }

    pub async fn acquire(&self, host: &str) -> (OwnedSemaphorePermit, OwnedSemaphorePermit) {
        let g = self.global.clone().acquire_owned().await.expect("semaphore");
        let hsem = self.host_sem(host).await;
        let h = hsem.acquire_owned().await.expect("semaphore");
        (g, h)
    }

    async fn host_polite_gate(&self, host: &str) -> Arc<tokio::sync::Mutex<Option<Instant>>> {
        let mut map = self.polite.lock().await;
        if let Some(g) = map.get(host) { return g.clone(); }
        let g = Arc::new(tokio::sync::Mutex::new(None));
        map.insert(host.to_string(), g.clone());
        g
    }

    /// Acquire permits while honoring an optional crawl-delay for the host.
    /// If `crawl_delay` is None, behaves like `acquire` (fast as possible).
    pub async fn acquire_polite(&self, host: &str, crawl_delay: Option<Duration>) -> (OwnedSemaphorePermit, OwnedSemaphorePermit) {
        if let Some(delay) = crawl_delay {
            let gate = self.host_polite_gate(host).await;
            let mut last = gate.lock().await;
            if let Some(prev) = *last {
                let now = Instant::now();
                let earliest = prev + delay;
                if let Some(wait) = earliest.checked_duration_since(now) {
                    if !wait.is_zero() {
                        tokio::time::sleep(wait).await;
                    }
                }
            }
            // record new timestamp to space subsequent calls
            *last = Some(Instant::now());
            drop(last);
        }
        self.acquire(host).await
    }
}
