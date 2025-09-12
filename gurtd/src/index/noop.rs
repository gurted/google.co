use anyhow::Result;

use crate::index::{IndexDocument, IndexEngine, SearchHit};
use crate::query::ParsedQuery;

#[derive(Default)]
pub struct NoopIndexEngine {
    pub docs_indexed: std::sync::atomic::AtomicU64,
}

impl IndexEngine for NoopIndexEngine {
    fn engine_name(&self) -> &'static str { "noop" }

    fn add(&self, _doc: IndexDocument) -> Result<()> {
        self.docs_indexed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn commit(&self) -> Result<()> { Ok(()) }

    fn refresh(&self) -> Result<()> { Ok(()) }

    fn search(&self, _query: &ParsedQuery, _page: usize, _size: usize) -> Result<Vec<SearchHit>> { Ok(Vec::new()) }
}
