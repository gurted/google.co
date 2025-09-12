use anyhow::Result;

use crate::query::ParsedQuery;

/// Minimal document representation for indexing, aligned with design.md.
#[derive(Debug, Clone)]
pub struct IndexDocument {
    pub url: String,
    pub domain: String,
    pub title: String,
    pub content: String,
    pub fetch_time: i64,
    pub language: String,    // e.g., "en"
    pub render_mode: String, // "static" | "rendered"
}

/// Minimal search hit representation for the query path.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub domain: String,
    pub fetch_time: i64,
    pub score: f32,
}

/// Pluggable index/search engine abstraction.
/// Implementations should be thread-safe; near-real-time updates are
/// expected via commit/refresh semantics.
pub trait IndexEngine: Send + Sync {
    fn engine_name(&self) -> &'static str;

    /// Add/replace a document in the index.
    fn add(&self, _doc: IndexDocument) -> Result<()>;

    /// Commit pending changes to make them durable.
    fn commit(&self) -> Result<()>;

    /// Refresh searchers to see new segments (near-real-time).
    fn refresh(&self) -> Result<()>;

    /// Execute a search with pagination.
    fn search(&self, _query: &ParsedQuery, _page: usize, _size: usize) -> Result<Vec<SearchHit>>;
}

pub mod tantivy;
pub mod noop;

/// Select an engine implementation by name.
pub fn make_engine(name: &str) -> anyhow::Result<Box<dyn IndexEngine>> {
    match name {
        "tantivy" => Ok(Box::new(tantivy::TantivyIndexEngine::with_default_schema())),
        "noop" => Ok(Box::new(noop::NoopIndexEngine::default())),
        other => Err(anyhow::anyhow!(format!("unknown engine: {}", other))),
    }
}
