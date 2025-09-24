use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use tokio::sync::OnceCell;
use tracing::{debug, info, warn};

pub use sqlx::PgPool;

pub mod tables {
    pub const DOMAINS: &str = "domains";
    pub const URLS: &str = "urls";
    pub const CRAWL_QUEUE: &str = "crawl_queue";
    pub const RECRAWL_QUEUE: &str = "recrawl_queue";
    pub const ROBOTS_CACHE: &str = "robots_cache";
    pub const FETCH_HISTORY: &str = "fetch_history";
    pub const LINK_EDGES: &str = "link_edges";
    pub const LINK_AUTHORITY: &str = "link_authority";
    pub const INDEX_SEGMENTS: &str = "index_segments";
    pub const QUERY_CACHE: &str = "query_cache";
    pub const RATE_LIMIT: &str = "rate_limit";
}

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone, Debug)]
pub struct DbConfig {
    pub database_url: Option<String>,

    pub min_connections: u32, // 0 (do not hold connections when idle)
    pub max_connections: u32, // 20
    pub connect_timeout_secs: u64, // 5
    pub idle_timeout_secs: Option<u64>, // None
    pub max_lifetime_secs: Option<u64>, // None
    pub acquire_timeout_secs: u64, // 5

    pub retry_max_attempts: u32, // 5
    pub retry_base_backoff_ms: u64, // 200

    /// true: will fail when the DB cannot be reached after retries.
    /// false: will log and continue; the first use of get_pool() will retry.
    pub eager_init: bool, // false

    /// true: run migrations after the first successful connect.
    /// ! DO NOT RUN ON PRODUCTION SYSTEMS UNLESS YOU KNOW WHAT YOU ARE DOING !
    pub migrate_on_start: bool, // false
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            database_url: None,
            min_connections: 0,
            max_connections: 20,
            connect_timeout_secs: 5,
            idle_timeout_secs: None,
            max_lifetime_secs: None,
            acquire_timeout_secs: 5,
            retry_max_attempts: 5,
            retry_base_backoff_ms: 200,
            eager_init: false,
            migrate_on_start: false,
        }
    }
}

impl DbConfig {
    /// - DATABASE_URL (optional)
    /// - DB_MIN_CONNECTIONS (default 0)
    /// - DB_MAX_CONNECTIONS (default 20)
    /// - DB_CONNECT_TIMEOUT_SECS (default 5)
    /// - DB_IDLE_TIMEOUT_SECS (optional)
    /// - DB_MAX_LIFETIME_SECS (optional)
    /// - DB_ACQUIRE_TIMEOUT_SECS (default 5)
    /// - DB_RETRY_MAX_ATTEMPTS (default 5)
    /// - DB_RETRY_BASE_BACKOFF_MS (default 200)
    /// - DB_EAGER_INIT (bool, default false)
    /// - DB_MIGRATE_ON_START (bool, default false)
    pub fn from_env() -> Self {
        let mut cfg = Self::default();

        cfg.database_url = std::env::var("DATABASE_URL").ok();

        cfg.min_connections = parse_env_u32("DB_MIN_CONNECTIONS", cfg.min_connections);
        cfg.max_connections = parse_env_u32("DB_MAX_CONNECTIONS", cfg.max_connections);
        cfg.connect_timeout_secs =
            parse_env_u64("DB_CONNECT_TIMEOUT_SECS", cfg.connect_timeout_secs);

        cfg.idle_timeout_secs = std::env::var("DB_IDLE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok());

        cfg.max_lifetime_secs = std::env::var("DB_MAX_LIFETIME_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok());

        cfg.acquire_timeout_secs =
            parse_env_u64("DB_ACQUIRE_TIMEOUT_SECS", cfg.acquire_timeout_secs);

        cfg.retry_max_attempts = parse_env_u32("DB_RETRY_MAX_ATTEMPTS", cfg.retry_max_attempts);

        cfg.retry_base_backoff_ms =
            parse_env_u64("DB_RETRY_BASE_BACKOFF_MS", cfg.retry_base_backoff_ms);

        cfg.eager_init = parse_env_bool("DB_EAGER_INIT", cfg.eager_init);
        cfg.migrate_on_start = parse_env_bool("DB_MIGRATE_ON_START", cfg.migrate_on_start);

        cfg
    }
}

pub struct Db {
    cfg: DbConfig,
    pool: OnceCell<PgPool>,
    migrated: OnceCell<()>,
}

impl Db {
    pub fn new(cfg: DbConfig) -> Self {
        Self {
            cfg,
            pool: OnceCell::new(),
            migrated: OnceCell::new(),
        }
    }

    /// - Eager mode: connect with retries and return error if unavailable.
    /// - Lazy mode: attempt connect with retries; if it fails, log a warning and continue.
    pub async fn init(&self) -> Result<(), DbInitError> {
        if self.cfg.eager_init {
            let pool = self.try_connect_with_retry().await?;

            let _ = self.pool.set(pool);
            if self.cfg.migrate_on_start {
                if let Some(pool) = self.pool.get() {
                    self.ensure_migrated(pool).await?;
                }
            }
        } else {
            match self.try_connect_with_retry().await {
                Ok(pool) => {
                    let _ = self.pool.set(pool);
                    if self.cfg.migrate_on_start {
                        if let Some(pool) = self.pool.get() {
                            self.ensure_migrated(pool).await?;
                        }
                    }
                }
                Err(e) => {
                    // Lazy: allow deferred connection
                    warn!(target: "gurt_db", "database not available at startup (lazy): {e}");
                }
            }
        }
        Ok(())
    }

    /// Get a connection pool, initializing it with retries on first use.
    /// If migrations are enabled, they run after the first successful connect.
    pub async fn get_pool(&self) -> Result<&PgPool, DbInitError> {
        // Fallible initialization of the OnceCell (connect with retry).
        let pool = self
            .pool
            .get_or_try_init(|| async { self.try_connect_with_retry().await })
            .await?;

        // Run migrations once after first connect if enabled.
        if self.cfg.migrate_on_start {
            self.ensure_migrated(pool).await?;
        }

        Ok(pool)
    }

    /// A quick status probe. Uses a short timeout to avoid hanging when the DB is degraded.
    pub async fn health_check(&self) -> HealthStatus {
        if self.cfg.database_url.is_none() {
            return HealthStatus::NoUrl;
        }
        let Some(pool) = self.pool.get() else {
            return HealthStatus::NotInitialized;
        };

        match tokio::time::timeout(
            Duration::from_secs(1),
            sqlx::query("SELECT 1").execute(pool),
        )
        .await
        {
            Ok(Ok(_)) => HealthStatus::Ok,
            Ok(Err(e)) => HealthStatus::Error(e.to_string()),
            Err(_) => HealthStatus::Error("health check timed out".to_string()),
        }
    }

    fn build_pool_options(&self) -> PgPoolOptions {
        let mut opts = PgPoolOptions::new()
            .min_connections(self.cfg.min_connections)
            .max_connections(self.cfg.max_connections)
            .acquire_timeout(Duration::from_secs(self.cfg.acquire_timeout_secs));

        if let Some(secs) = self.cfg.idle_timeout_secs {
            opts = opts.idle_timeout(Duration::from_secs(secs));
        }
        if let Some(secs) = self.cfg.max_lifetime_secs {
            opts = opts.max_lifetime(Duration::from_secs(secs));
        }
        opts
    }

    async fn try_connect_with_retry(&self) -> Result<PgPool, DbInitError> {
        let url = match self.cfg.database_url.as_deref() {
            Some(u) => u,
            None => return Err(DbInitError::MissingUrl),
        };

        let max = self.cfg.retry_max_attempts.max(1);
        let connect_timeout_secs = self.cfg.connect_timeout_secs;

        let mut last_err: Option<String> = None;
        for attempt in 1..=max {
            let connect_future = self.build_pool_options().connect(url);
            let result = if connect_timeout_secs > 0 {
                match tokio::time::timeout(
                    Duration::from_secs(connect_timeout_secs),
                    connect_future,
                )
                .await
                {
                    Ok(inner) => inner.map_err(|e| e.to_string()),
                    Err(_) => Err(format!(
                        "connect attempt timed out after {}s",
                        connect_timeout_secs
                    )),
                }
            } else {
                connect_future.await.map_err(|e| e.to_string())
            };

            match result {
                Ok(pool) => {
                    debug!(
                        target = "gurt_db",
                        "connected to database on attempt {}", attempt
                    );
                    return Ok(pool);
                }
                Err(msg) => {
                    last_err = Some(msg.clone());
                    if attempt >= max {
                        break;
                    }
                    let delay = compute_backoff_ms(self.cfg.retry_base_backoff_ms, attempt);
                    warn!(
                        target = "gurt_db",
                        "db connect attempt {}/{} failed: {} ; retrying in {} ms",
                        attempt,
                        max,
                        msg,
                        delay
                    );
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
            }
        }

        Err(DbInitError::Connect {
            attempts: max,
            last_error: last_err.unwrap_or_else(|| "unknown error".to_string()),
        })
    }

    async fn ensure_migrated(&self, pool: &PgPool) -> Result<(), DbInitError> {
        self.migrated
            .get_or_try_init(|| async {
                info!(target = "gurt_db", "running database migrations");
                MIGRATOR
                    .run(pool)
                    .await
                    .map(|_| ())
                    .map_err(|e| DbInitError::Migrate(e.to_string()))
            })
            .await
            .map(|_| ())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DbInitError {
    #[error("DATABASE_URL is not set")]
    MissingUrl,

    #[error("failed to connect after {attempts} attempt(s): {last_error}")]
    Connect { attempts: u32, last_error: String },

    #[error("migrations failed: {0}")]
    Migrate(String),

    #[error("unexpected error: {0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    NoUrl,
    NotInitialized,
    Ok,
    Error(String),
}

fn parse_env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(default)
}

fn parse_env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(default)
}

fn parse_env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|s| {
            let s = s.trim().to_ascii_lowercase();
            matches!(s.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(default)
}

fn compute_backoff_ms(base_ms: u64, attempt: u32) -> u64 {
    let mut factor = 1u64;
    for _ in 1..attempt {
        factor = factor.saturating_mul(2);
    }
    let capped = (base_ms.saturating_mul(factor)).min(30_000);
    let jitter = fastrand::u64(0..(base_ms / 2 + 1));
    capped.saturating_add(jitter)
}
