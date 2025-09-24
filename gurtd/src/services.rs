use once_cell::sync::{Lazy, OnceCell};

use gurt_db::PgPool;

use crate::index::{make_engine, IndexEngine};

#[derive(Debug)]
pub struct Services {
    db_pool: PgPool,
}

impl Services {
    pub fn db(&self) -> &PgPool {
        &self.db_pool
    }

    pub fn index_engine(&self) -> &'static dyn IndexEngine {
        &**INDEX_ENGINE
    }
}

/// Global index engine instance shared across the server.
static INDEX_ENGINE: Lazy<Box<dyn IndexEngine>> = Lazy::new(|| {
    // prefer on-disk Tantivy when GURT_INDEX_DIR is set, else fall back to in-memory engine
    if let Ok(dir) = std::env::var("GURT_INDEX_DIR") {
        let path = dir.trim();
        if !path.is_empty() {
            match crate::index::tantivy::TantivyIndexEngine::open_or_create_in_dir(path) {
                Ok(engine) => {
                    eprintln!("[index] using Tantivy on-disk index at {}", path);
                    return Box::new(engine);
                }
                Err(e) => {
                    eprintln!(
                        "[index] failed to open Tantivy index at {}: {:?} ; falling back to in-memory",
                        path, e
                    );
                }
            }
        }
    }
    make_engine("tantivy")
        .or_else(|_| make_engine("noop"))
        .expect("index engine")
});

static SERVICES: OnceCell<Services> = OnceCell::new();

pub fn init(db_pool: PgPool) {
    SERVICES
        .set(Services { db_pool })
        .expect("services already initialized");
}

pub fn services() -> &'static Services {
    SERVICES.get().expect("services not initialized")
}

pub fn db() -> &'static PgPool {
    services().db()
}

/// Obtain a reference to the global index engine.
pub fn index_engine() -> &'static dyn IndexEngine {
    &**INDEX_ENGINE
}
