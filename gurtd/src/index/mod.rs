pub use gurt_index::{noop, tantivy};
pub use gurt_index::{IndexDocument, IndexEngine, SearchHit};

pub fn make_engine(name: &str) -> anyhow::Result<Box<dyn IndexEngine>> {
    gurt_index::register_defaults();
    gurt_index::make_engine(name)
        .ok_or_else(|| anyhow::anyhow!(format!("unknown engine: {}", name)))
}
