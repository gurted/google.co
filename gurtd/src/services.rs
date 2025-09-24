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
