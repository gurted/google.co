use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing;
use tokio::time;

use anyhow::{anyhow, Result};

use crate::crawler::pipeline::DynamicReCrawlQueue;
use crate::crawler::sitemap::parse_sitemap_xml;
use crate::services;

mod dns;
mod fetch;

const DEFAULT_PORT: u16 = 4878;
const MAX_PAGES_PER_DOMAIN: usize = 16;
const RENDER_BUDGET: std::time::Duration = std::time::Duration::from_millis(120);

/// Public entry point used by the router when a new domain submission arrives.
pub fn enqueue_domain(domain: String) {
    if domain.is_empty() {
        return;
    }
    INDEXING_SERVICE.enqueue(domain);
}

static INDEXING_SERVICE: Lazy<IndexingService> = Lazy::new(IndexingService::new);
static RECRAWL_QUEUE: Lazy<DynamicReCrawlQueue> = Lazy::new(DynamicReCrawlQueue::new);

struct IndexingService {
    sender: Mutex<Option<UnboundedSender<IndexJob>>>,
    in_flight: Arc<Mutex<HashSet<String>>>,
}

impl IndexingService {
    fn new() -> Self {
        Self {
            sender: Mutex::new(None),
            in_flight: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn enqueue(&self, domain: String) {
        {
            let mut guard = self.in_flight.lock().unwrap();
            if guard.contains(&domain) {
                return;
            }
            guard.insert(domain.clone());
        }
        let sender = self.ensure_worker();
        if sender
            .send(IndexJob {
                domain: domain.clone(),
            })
            .is_err()
        {
            let mut guard = self.in_flight.lock().unwrap();
            guard.remove(&domain);
        }
    }

    fn ensure_worker(&self) -> UnboundedSender<IndexJob> {
        let mut guard = self.sender.lock().unwrap();
        if let Some(tx) = guard.as_ref() {
            return tx.clone();
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let in_flight = self.in_flight.clone();
        std::thread::Builder::new()
            .name("gurt-indexer".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("indexing runtime");
                runtime.block_on(run_worker(rx, in_flight));
            })
            .expect("spawn indexing worker");
        *guard = Some(tx.clone());
        tx
    }
}

struct IndexJob {
    domain: String,
}

async fn run_worker(mut rx: UnboundedReceiver<IndexJob>, in_flight: Arc<Mutex<HashSet<String>>>) {
    while let Some(job) = rx.recv().await {
        if let Err(err) = process_domain(&job.domain).await {
            eprintln!("[indexing] domain={} error={:?}", job.domain, err);
        }
        let mut guard = in_flight.lock().unwrap();
        guard.remove(&job.domain);
    }
}

async fn process_domain(domain: &str) -> Result<()> {
    eprintln!("[indexing] enqueue domain={}", domain);
    let urls = collect_candidate_urls(domain).await;
    if urls.is_empty() {
        return Err(anyhow!("no crawl candidates"));
    }

    for url in urls {
        if let Err(err) = fetch::index_single_url(&url, &RECRAWL_QUEUE).await {
            eprintln!("[indexing] url={} error={:?}", url, err);
        }
    }

    let engine = services::index_engine();
    if let Err(err) = engine.commit() {
        eprintln!("[indexing] commit error: {err:?}");
    }
    if let Err(err) = engine.refresh() {
        eprintln!("[indexing] refresh error: {err:?}");
    }

    let queued = RECRAWL_QUEUE.len().await;
    if queued > 0 {
        let drained = RECRAWL_QUEUE.drain().await;
        for item in drained {
            eprintln!(
                "[indexing] dynamic requeue url={} reason={:?}",
                item.url, item.reason
            );
        }
    }

    // mark domain as ready in DB reliably with retries
    let pool = services::db().clone();
    let mut attempts = 0;
    const MAX_ATTEMPTS: usize = 3;
    let mut backoff = std::time::Duration::from_millis(100);
    loop {
        attempts += 1;
        match crate::storage::domains::set_domain_status(&pool, domain, "ready").await {
            Ok(_) => break,
            Err(err) => {
                if attempts >= MAX_ATTEMPTS {
                    tracing::error!("[indexing] failed to set domain={} to ready after {} attempts: {:?}", domain, attempts, err);
                    break;
                }
                tracing::warn!("[indexing] retrying set_domain_status for domain={} (attempt {}/{}) due to: {:?}", domain, attempts, MAX_ATTEMPTS, err);
                tokio::time::sleep(backoff).await;
                backoff = backoff * 2;
            }
        }
    }

    Ok(())
}

async fn collect_candidate_urls(domain: &str) -> Vec<String> {
    let mut urls = vec![format!("gurt://{domain}/")];
    let sitemap_url = format!("gurt://{domain}/sitemap.xml");
    if let Ok(resp) = fetch::fetch_gurt(&sitemap_url).await {
        if (200..300).contains(&resp.code) {
            if let Ok(xml) = String::from_utf8(resp.body.clone()) {
                let entries = parse_sitemap_xml(&xml);
                for entry in entries {
                    if urls.len() >= MAX_PAGES_PER_DOMAIN {
                        break;
                    }
                    if let Some(normalized) = normalize_candidate_url(domain, entry) {
                        urls.push(normalized);
                    }
                }
            }
        }
    }
    urls.sort();
    urls.dedup();
    urls.truncate(MAX_PAGES_PER_DOMAIN);
    urls
}

fn normalize_candidate_url(domain: &str, raw: String) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("gurt://") {
        return Some(trimmed.to_string());
    }
    if trimmed.starts_with('/') {
        return Some(format!("gurt://{}{}", domain, trimmed));
    }
    Some(format!("gurt://{}/{trimmed}", domain))
}
