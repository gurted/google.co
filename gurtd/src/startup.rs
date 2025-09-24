// TODO bootstrap v2 notes:
// - when enabling a DB-backed crawl queue (env GURT_USE_DB_QUEUE=1), clear stale locks:
//   UPDATE crawl_queue SET locked_by = NULL, locked_at = NULL
//   WHERE locked_at IS NOT NULL AND locked_at &lt; NOW() - interval '5 minutes';
//   consider a tunable window via GURT_QUEUE_LOCK_STALE_SECS
// - apply the same policy to recrawl_queue if leasing is added there
// - prefer leasing URL-level jobs from DB rather than re-enqueuing whole domains to avoid refetch churn
// - in multi-server mode, run bootstrap in a single coordinator and shard work by domain hash
// - keep bootstrap bounded and non-blocking; always cap with GURT_BOOTSTRAP_LIMIT and sparse progress logs
use anyhow::Result;
use std::time::Instant;

fn env_flag_true(key: &str, default_true: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => {
            let s = v.trim().to_ascii_lowercase();
            matches!(s.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => default_true,
    }
}

fn env_usize(key: &str, default_val: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(default_val)
}

/// Fast-path notes:
/// - non-blocking of accept loop: caller should spawn this (see main.rs)
/// - bounded work via GURT_BOOTSTRAP_LIMIT (default 200) to keep startup snappy
/// - progress logs are sparse (every GURT_BOOTSTRAP_LOG_EVERY; default 50)
pub async fn bootstrap_resume() -> Result<()> {
    if !env_flag_true("GURT_BOOTSTRAP_ENABLED", true) {
        eprintln!("[bootstrap] disabled via GURT_BOOTSTRAP_ENABLED");
        return Ok(());
    }

    let start = Instant::now();
    let pool = crate::services::db().clone();
    let limit = env_usize("GURT_BOOTSTRAP_LIMIT", 200);
    let log_every = env_usize("GURT_BOOTSTRAP_LOG_EVERY", 50);

    let domains = match crate::storage::domains::list_pending_domains(&pool, limit as i64).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[bootstrap] list_pending_domains error: {:?}", e);
            Vec::new()
        }
    };

    let total = domains.len();
    if total == 0 {
        eprintln!(
            "[bootstrap] no pending domains; took {:?}",
            start.elapsed()
        );
        return Ok(());
    }

    for (i, d) in domains.into_iter().enumerate() {
        crate::indexing::enqueue_domain(d);
        let done = i + 1;
        if log_every > 0 && (done % log_every == 0 || done == total) {
            eprintln!(
                "[bootstrap] enqueued {}/{} domains in {:?}",
                done,
                total,
                start.elapsed()
            );
        }
    }

    Ok(())
}