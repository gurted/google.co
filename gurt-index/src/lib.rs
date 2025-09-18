use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use anyhow::Result;
use gurt_query::ParsedQuery;

/// Minimal document representation for indexing.
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

/// Minimal search hit representation.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub domain: String,
    pub fetch_time: i64,
    pub score: f32,
}

/// Pluggable index/search engine abstraction.
pub trait IndexEngine: Send + Sync {
    fn engine_name(&self) -> &'static str;

    /// Add/replace a document in the index.
    fn add(&self, doc: IndexDocument) -> Result<()>;

    /// Commit pending changes to make them durable.
    fn commit(&self) -> Result<()>;

    /// Refresh searchers to see new segments (near-real-time).
    fn refresh(&self) -> Result<()>;

    /// Execute a search with pagination.
    fn search(&self, query: &ParsedQuery, page: usize, size: usize) -> Result<Vec<SearchHit>>;
}

type EngineFactory = fn() -> Box<dyn IndexEngine>;

static REGISTRY: OnceLock<Mutex<HashMap<&'static str, EngineFactory>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<&'static str, EngineFactory>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register an engine factory under a name. Typically invoked by a macro.
pub fn register_engine(name: &'static str, factory: EngineFactory) {
    let map = registry();
    let mut lock = map.lock().expect("engine registry poisoned");
    lock.insert(name, factory);
}

/// Create an engine instance by name if registered.
pub fn make_engine(name: &str) -> Option<Box<dyn IndexEngine>> {
    let map = registry();
    let lock = map.lock().expect("engine registry poisoned");
    lock.get(name).map(|f| f())
}

/// List registered engines.
pub fn list_engines() -> Vec<String> {
    let map = registry();
    let lock = map.lock().expect("engine registry poisoned");
    lock.keys().map(|k| (*k).to_string()).collect()
}
pub mod noop;
pub mod tantivy;

pub fn register_defaults() {
    tantivy::register();
    noop::register();
}
