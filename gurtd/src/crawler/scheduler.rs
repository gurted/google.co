use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore, OwnedSemaphorePermit};

#[derive(Clone)]
pub struct HostScheduler {
    global: Arc<Semaphore>,
    per_host_limit: usize,
    hosts: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
}

impl HostScheduler {
    pub fn new(global_limit: usize, per_host_limit: usize) -> Self {
        Self {
            global: Arc::new(Semaphore::new(global_limit)),
            per_host_limit,
            hosts: Arc::new(Mutex::new(HashMap::new())),
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
}

