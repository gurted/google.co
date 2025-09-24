use anyhow::Result;
use once_cell::sync::Lazy;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

use gurt_api::response::SearchResponse;
use gurt_api::status::StatusCode;

use crate::indexing;
use crate::proto::http_like::{Request, Response};
use crate::query::parse_query;
use crate::search::{normalize_key, HotQueryCache};

use super::search_utils::rescore_and_convert;
use super::util::{json_response, percent_decode};

static HOT_CACHE: Lazy<HotQueryCache> =
    Lazy::new(|| HotQueryCache::new(std::time::Duration::from_secs(20)));

pub fn handle_search(req: Request) -> Result<Response> {
    // Minimal parse for q param; page/size defaults
    let mut q = None;
    if let Some(query) = req.query() {
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == "q" {
                    q = Some(percent_decode(v));
                }
            }
        }
    }
    let q = q.unwrap_or_default();
    if q.trim().is_empty() {
        return Ok(Response {
            code: StatusCode::BadRequest,
            headers: vec![],
            body: vec![],
        });
    }
    // Overload and internal error mapping (stubbed via env flags for now)
    if std::env::var("GURT_OVERLOADED")
        .ok()
        .filter(|v| v != "0")
        .is_some()
    {
        return Ok(Response {
            code: StatusCode::TooManyRequests,
            headers: vec![],
            body: vec![],
        });
    }
    if std::env::var("GURT_FORCE_500")
        .ok()
        .filter(|v| v != "0")
        .is_some()
    {
        return Ok(Response {
            code: StatusCode::InternalServerError,
            headers: vec![],
            body: vec![],
        });
    }
    // Query cache: normalize q+filters
    let pq = parse_query(&q);
    let key = normalize_key(&pq);
    if let Some(hit) = HOT_CACHE.get(&key) {
        let body = serde_json::to_vec(&hit).unwrap_or_else(|_| b"{}".to_vec());
        return Ok(json_response(StatusCode::Ok, body));
    }

    // Execute query on the default engine.
    // TODO: thread pagination from the client once the UI grows controls.
    let page = 1usize;
    let size = 10usize;
    let engine = crate::services::index_engine();
    let hits = engine.search(&pq, page, size).unwrap_or_default();
    // Rescore BM25 -> link -> trust -> recency
    let results = rescore_and_convert(hits, size as usize);
    let resp = SearchResponse {
        query: pq.terms.join(" "),
        total: results.len() as u64,
        page: page as u32,
        size: size as u32,
        results,
    };
    HOT_CACHE.put(key, resp.clone());
    let body = serde_json::to_vec(&resp).unwrap_or_else(|_| b"{}".to_vec());
    Ok(json_response(StatusCode::Ok, body))
}

// rescoring is handled in search_utils::rescore_and_convert

// Simple in-memory submissions store and IP rate limiter for POST /api/sites
static SUBMITTED_SITES: Lazy<std::sync::Mutex<std::collections::HashSet<String>>> =
    Lazy::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

struct IpRateLimiter {
    max: usize,
    window: std::time::Duration,
    map: std::sync::Mutex<
        std::collections::HashMap<IpAddr, std::collections::VecDeque<std::time::Instant>>,
    >,
}

impl IpRateLimiter {
    fn new(max: usize, window: std::time::Duration) -> Self {
        Self {
            max,
            window,
            map: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
    fn allow(&self, ip: IpAddr) -> bool {
        let now = std::time::Instant::now();
        let mut map = self.map.lock().unwrap();
        let q = map
            .entry(ip)
            .or_insert_with(|| std::collections::VecDeque::new());
        while let Some(&t) = q.front() {
            if now.duration_since(t) > self.window {
                q.pop_front();
            } else {
                break;
            }
        }
        if q.len() < self.max {
            q.push_back(now);
            true
        } else {
            false
        }
    }
}

static RATE_LIMITER: Lazy<IpRateLimiter> = Lazy::new(|| {
    let max = std::env::var("GURT_SUBMIT_RATE")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(5);
    let win = std::env::var("GURT_SUBMIT_WINDOW")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(60);
    IpRateLimiter::new(max, std::time::Duration::from_secs(win))
});

pub fn handle_add_site(req: Request, peer: Option<SocketAddr>) -> Result<Response> {
    // Determine client IP (peer preferred, fallback to x-forwarded-for)
    let ip_from_peer = peer.map(|p| p.ip());
    let ip_from_header = super::util::get_header(&req, "x-forwarded-for")
        .and_then(|s| s.split(',').next())
        .and_then(|s| IpAddr::from_str(s.trim()).ok());
    let ip = ip_from_peer.or(ip_from_header);
    if let Some(ip) = ip {
        if !RATE_LIMITER.allow(ip) {
            return Ok(Response {
                code: StatusCode::TooManyRequests,
                headers: vec![],
                body: vec![],
            });
        }
    }

    // Parse and validate body
    let domain = extract_domain_from_body(&req.body).unwrap_or_default();
    if domain.is_empty() {
        return Ok(Response {
            code: StatusCode::BadRequest,
            headers: vec![],
            body: vec![],
        });
    }

    {
        let mut set = SUBMITTED_SITES.lock().unwrap();
        set.insert(domain.clone());
    }

    // persist submission asynchronously to DB (fire-and-forget to keep latency low)
    {
        let pool = crate::services::db().clone();
        let d = domain.clone();
        tokio::spawn(async move {
            let _ = crate::storage::domains::upsert_domain_submission(&pool, &d, Some("api")).await;
        });
    }

    indexing::enqueue_domain(domain.clone());

    let body = serde_json::to_vec(&serde_json::json!({
        "status": "accepted",
        "domain": domain
    }))
    .unwrap_or_else(|_| b"{}".to_vec());
    Ok(json_response(StatusCode::Ok, body))
}

fn extract_domain_from_body(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let mut domain = if let Some(d) = v.get("domain").and_then(|x| x.as_str()) {
        d.to_string()
    } else if let Some(u) = v.get("url").and_then(|x| x.as_str()) {
        if let Some(rest) = u.strip_prefix("gurt://") {
            rest.split('/').next().unwrap_or("").to_string()
        } else {
            u.to_string()
        }
    } else {
        String::new()
    };
    domain = domain.trim().to_lowercase();
    if !is_valid_domain(&domain) {
        return None;
    }
    Some(domain)
}

fn is_valid_domain(s: &str) -> bool {
    if s.is_empty() || s.len() > 255 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
}
