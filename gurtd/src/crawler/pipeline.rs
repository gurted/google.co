use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

use crate::crawler::render::{render_once, DynamicReason, RenderConfig};
use crate::index::{IndexDocument, IndexEngine};

/// Item representing a dynamic page that timed out during render and should be re-crawled/rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReCrawlItem {
    pub url: String,
    pub reason: Option<DynamicReason>,
}

/// A simple in-memory queue/log for pages to re-crawl/re-render.
#[derive(Default, Clone)]
pub struct DynamicReCrawlQueue {
    inner: Arc<tokio::sync::Mutex<Vec<ReCrawlItem>>>,
}

impl DynamicReCrawlQueue {
    pub fn new() -> Self { Self::default() }

    pub async fn enqueue(&self, item: ReCrawlItem) {
        let mut q = self.inner.lock().await;
        q.push(item);
    }

    pub async fn drain(&self) -> Vec<ReCrawlItem> {
        let mut q = self.inner.lock().await;
        let out = q.clone();
        q.clear();
        out
    }

    pub async fn len(&self) -> usize { self.inner.lock().await.len() }
}

/// Process a fetched HTML document through the selective render-once pipeline
/// and add it to the index. If the dynamic render path times out, enqueue the
/// URL for re-crawl/re-render.
pub async fn process_fetched_document(
    engine: &dyn IndexEngine,
    requeue: &DynamicReCrawlQueue,
    url: &str,
    domain: &str,
    title: &str,
    html: &str,
    language: &str,
    fetch_time: i64,
    render_budget: Duration,
) -> Result<()> {
    process_fetched_document_with_cost(engine, requeue, url, domain, title, html, language, fetch_time, render_budget, None).await
}

/// Same as `process_fetched_document` but allows overriding a simulated render cost
/// (for tests). If `simulated_cost` > `render_budget`, the pipeline will mark timeout.
pub async fn process_fetched_document_with_cost(
    engine: &dyn IndexEngine,
    requeue: &DynamicReCrawlQueue,
    url: &str,
    domain: &str,
    title: &str,
    html: &str,
    language: &str,
    fetch_time: i64,
    render_budget: Duration,
    simulated_cost: Option<Duration>,
) -> Result<()> {
    let cfg = RenderConfig { time_budget: render_budget, simulated_cost };
    let outcome = render_once(html, &cfg).await;

    if outcome.timed_out {
        requeue.enqueue(ReCrawlItem { url: url.to_string(), reason: outcome.reason.clone() }).await;
        // Log for visibility (stdout)
        eprintln!("[crawler] render timeout: url={} reason={:?}", url, outcome.reason);
    }

    let doc = IndexDocument {
        url: url.to_string(),
        domain: domain.to_string(),
        title: title.to_string(),
        content: outcome.content,
        fetch_time,
        language: language.to_string(),
        render_mode: outcome.render_mode,
    };
    engine.add(doc)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::ParsedQuery;
    use crate::index::{SearchHit};
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct TestEngine { last: StdMutex<Option<IndexDocument>> }
    impl IndexEngine for TestEngine {
        fn engine_name(&self) -> &'static str { "test" }
        fn add(&self, doc: IndexDocument) -> Result<()> { *self.last.lock().unwrap() = Some(doc); Ok(()) }
        fn commit(&self) -> Result<()> { Ok(()) }
        fn refresh(&self) -> Result<()> { Ok(()) }
        fn search(&self, _q: &ParsedQuery, _p: usize, _s: usize) -> Result<Vec<SearchHit>> { Ok(vec![]) }
    }

    #[tokio::test]
    async fn pipeline_renders_dynamic_under_budget() {
        let eng = TestEngine::default();
        let queue = DynamicReCrawlQueue::new();
        let html = "<html><body><script type=\"text/lua\">print(1)</script><div>ok</div></body></html>";
        process_fetched_document(&eng, &queue, "gurt://ex/a", "ex", "t", html, "en", 1_000, Duration::from_millis(20)).await.unwrap();
        let doc = eng.last.lock().unwrap().clone().unwrap();
        assert_eq!(doc.render_mode, "rendered");
        assert!(!doc.content.contains("<script"));
        assert_eq!(queue.len().await, 0);
    }

    #[tokio::test]
    async fn pipeline_enqueues_on_timeout_and_indexes_static() {
        // Force timeout by using a large simulated cost via detect path: we can’t set simulated_cost here,
        // so instead wrap render_once with an override by calling with a tiny budget and a long sleep
        // Simulate latency by providing simulated_cost > budget.
        let eng = TestEngine::default();
        let queue = DynamicReCrawlQueue::new();
        // network.fetch triggers dynamic path; with zero budget, render_once will return static with timed_out=true
        let html = "<div>call network.fetch(\"/api\")</div>";
        process_fetched_document_with_cost(&eng, &queue, "gurt://ex/b", "ex", "t", html, "en", 1_001, Duration::from_millis(5), Some(Duration::from_millis(25))).await.unwrap();
        let doc = eng.last.lock().unwrap().clone().unwrap();
        assert_eq!(doc.render_mode, "static");
        assert_eq!(doc.content, html);
        assert_eq!(queue.len().await, 1);
        let drained = queue.drain().await;
        assert_eq!(drained[0].url, "gurt://ex/b");
        assert!(matches!(drained[0].reason, Some(DynamicReason::NetworkFetch)));
    }

    #[tokio::test]
    async fn pipeline_static_passthrough_no_queue() {
        let eng = TestEngine::default();
        let queue = DynamicReCrawlQueue::new();
        let html = "<html><body>static</body></html>";
        process_fetched_document(&eng, &queue, "gurt://ex/c", "ex", "t", html, "en", 1_002, Duration::from_millis(10)).await.unwrap();
        let doc = eng.last.lock().unwrap().clone().unwrap();
        assert_eq!(doc.render_mode, "static");
        assert_eq!(doc.content, html);
        assert_eq!(queue.len().await, 0);
    }
}
