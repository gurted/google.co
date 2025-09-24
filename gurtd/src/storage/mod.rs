// NOTE: keep operations minimal and async to avoid blocking request paths.
// TODO: evolve toward DB-backed crawl queue and multi-server operation.

use anyhow::Result;
use gurt_db::PgPool;

pub mod domains {
    use super::*;
    use sqlx::Row;

    // Insert or update a domain submission.
    // - Normalizes to lowercase.
    // - Ensures status is 'pending' on first insert.
    // - On duplicate, bumps updated_at and preserves existing status unless you change it elsewhere.
    // Returns the domain id.
    pub async fn upsert_domain_submission(
        pool: &PgPool,
        name: &str,
        submission_source: Option<&str>,
    ) -> Result<i64> {
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() {
            anyhow::bail!("empty domain");
        }
        // Use UNIQUE(name) for conflict; schema also contains UNIQUE index on LOWER(name).
        let row = sqlx::query(
            "INSERT INTO domains (name, submission_source, status)
             VALUES ($1, $2, 'pending')
             ON CONFLICT (name)
             DO UPDATE SET
               updated_at = CURRENT_TIMESTAMP,
               submission_source = COALESCE(EXCLUDED.submission_source, domains.submission_source)
             RETURNING id",
        )
        .bind(&name)
        .bind(submission_source)
        .fetch_one(pool)
        .await?;
        let id: i64 = row.try_get("id")?;
        Ok(id)
    }

    // FIFO set of pending domains on submitted_at.
    pub async fn list_pending_domains(pool: &PgPool, limit: i64) -> Result<Vec<String>> {
        let limit = if limit <= 0 { 0 } else { limit.min(10_000) }; // cap hard to avoid surprise load
        if limit == 0 {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT name
               FROM domains
              WHERE status = 'pending'
              ORDER BY submitted_at ASC
              LIMIT $1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let name: String = r.try_get("name")?;
            out.push(name);
        }
        Ok(out)
    }

    // Optional: update a domain status explicitly.
    // Useful for future workflows when moving to a DB-backed queue.
    pub async fn set_domain_status(pool: &PgPool, name: &str, status: &str) -> Result<()> {
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() {
            return Ok(());
        }
        // Rely on enum domain_status in the schema; Postgres will validate.
        let _ = sqlx::query(
            "UPDATE domains
                SET status = $2, updated_at = CURRENT_TIMESTAMP
              WHERE LOWER(name) = LOWER($1)",
        )
        .bind(&name)
        .bind(status)
        .execute(pool)
        .await?;
        Ok(())
    }
}

pub mod queue {
    // scaffolding for v2 DB-backed crawl queue kept here with TODOs for future work
    // current indexer enqueues in-memory and commits to Tantivy directly
    // TODO: implement urls + crawl_queue population and leasing

    #![allow(dead_code)]
    // use sqlx::Row;

    // Example signature for future use:
    // pub async fn enqueue_url(pool: &PgPool, domain_id: i64, canonical_url: &str, priority: i32) -> Result<i64> {
    //     // TODO: compute normalized_hash, insert into urls, then into crawl_queue (ON CONFLICT DO NOTHING)
    //     // Return url_id
    //     unimplemented!()
    // }

    // pub async fn lease_next(pool: &PgPool, worker_id: &str) -> Result<Option<(i64 /*url_id*/, String /*url*/)>> {
    //     // TODO: SELECT ... FOR UPDATE SKIP LOCKED
    //     unimplemented!()
    // }

    // pub async fn clear_stale_locks(pool: &PgPool, older_than_seconds: i64) -> Result<u64> {
    //     // TODO: UPDATE crawl_queue SET locked_by = NULL, locked_at = NULL WHERE ... RETURNING count
    //     unimplemented!()
    // }
}