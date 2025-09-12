use anyhow::Result;
use gurt_api::status::StatusCode;
use gurt_api::response::{SearchResponse, SearchResultItem};
use serde_json;
use crate::proto::http_like::{Request, Response};
use crate::query::parse_query;
use crate::search::{HotQueryCache, normalize_key, merge_topk};
use crate::index::{make_engine, IndexEngine, SearchHit};
use crate::link::{AuthorityStore, domain_trust_from_cname_depth};

use once_cell::sync::Lazy;

static HOT_CACHE: Lazy<HotQueryCache> = Lazy::new(|| HotQueryCache::new(std::time::Duration::from_secs(20)));
static ENGINE: Lazy<Box<dyn IndexEngine>> = Lazy::new(|| make_engine("tantivy").expect("engine"));
static AUTH_STORE: Lazy<std::sync::Mutex<AuthorityStore>> = Lazy::new(|| std::sync::Mutex::new(AuthorityStore::new()));

pub fn handle(req: Request) -> Result<Response> {
    match (req.method.as_str(), req.path.split('?').next().unwrap_or("")) {
        ("GET", "/health/ready") => Ok(json_response(StatusCode::Ok, b"{\"status\":\"ready\"}".to_vec())),
        ("GET", "/search") => handle_search(req),
        _ => Ok(Response { code: StatusCode::BadRequest, headers: vec![], body: vec![] }),
    }
}

fn handle_search(req: Request) -> Result<Response> {
    // Minimal parse for q param; page/size defaults
    let mut q = None;
    if let Some(query) = req.query() {
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == "q" { q = Some(percent_decode(v)); }
            }
        }
    }
    let q = q.unwrap_or_default();
    if q.trim().is_empty() {
        return Ok(Response { code: StatusCode::BadRequest, headers: vec![], body: vec![] });
    }
    // Overload and internal error mapping (stubbed via env flags for now)
    if std::env::var("GURT_OVERLOADED").ok().filter(|v| v != "0").is_some() {
        return Ok(Response { code: StatusCode::TooManyRequests, headers: vec![], body: vec![] });
    }
    if std::env::var("GURT_FORCE_500").ok().filter(|v| v != "0").is_some() {
        return Ok(Response { code: StatusCode::InternalServerError, headers: vec![], body: vec![] });
    }
    // Query cache: normalize q+filters
    let pq = parse_query(&q);
    let key = normalize_key(&pq);
    if let Some(hit) = HOT_CACHE.get(&key) {
        let body = serde_json::to_vec(&hit).unwrap_or_else(|_| b"{}".to_vec());
        return Ok(json_response(StatusCode::Ok, body));
    }

    // Execute query on default engine
    let page = 1usize; let size = 10usize;
    let hits = ENGINE.search(&pq, page, size).unwrap_or_default();
    // Rescore BM25 -> link -> trust -> recency
    let results = rescore_and_convert(hits, size as usize);
    let resp = SearchResponse { query: pq.terms.join(" "), total: results.len() as u64, page: page as u32, size: size as u32, results };
    HOT_CACHE.put(key, resp.clone());
    let body = serde_json::to_vec(&resp).unwrap_or_else(|_| b"{}".to_vec());
    Ok(json_response(StatusCode::Ok, body))
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .to_string()
}

fn json_response(code: StatusCode, body: Vec<u8>) -> Response {
    Response { code, headers: vec![("content-type".into(), "application/json".into())], body }
}

fn rescore_and_convert(hits: Vec<SearchHit>, k: usize) -> Vec<SearchResultItem> {
    if hits.is_empty() { return Vec::new(); }
    let max_bm = hits.iter().map(|h| h.score).fold(0.0f32, |a, b| a.max(b)).max(1e-6);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let half_life_secs = 7 * 24 * 3600i64; // 7 days
    let weights = (0.6f64, 0.2f64, 0.1f64, 0.1f64); // (bm25, authority, trust, recency)
    let store = AUTH_STORE.lock().unwrap();
    let mut rescored: Vec<SearchResultItem> = hits.into_iter().map(|h| {
        let bm25 = (h.score / max_bm) as f64;
        let auth = store.get(&h.url).unwrap_or(0.0) as f64;
        let trust = domain_trust_from_cname_depth(0); // TODO: integrate real CNAME depth when available
        let age = (now - h.fetch_time).max(0) as f64;
        let recency = if half_life_secs > 0 { (0.5f64).powf(age / (half_life_secs as f64)) } else { 0.0 };
        let score = weights.0 * bm25 + weights.1 * auth + weights.2 * trust + weights.3 * recency;
        SearchResultItem { title: h.title, url: h.url, score: score as f32 }
    }).collect();
    // Merge top-k (single shard here) and sort by score desc
    rescored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let merged = merge_topk(vec![rescored], k);
    merged
}
