use once_cell::sync::Lazy;

use crate::index::{make_engine, IndexEngine};

/// Global index engine instance shared across the server.
static INDEX_ENGINE: Lazy<Box<dyn IndexEngine>> = Lazy::new(|| {
    make_engine("tantivy")
        .or_else(|_| make_engine("noop"))
        .expect("index engine")
});

/// Obtain a reference to the global index engine.
pub fn index_engine() -> &'static dyn IndexEngine {
    &**INDEX_ENGINE
}
