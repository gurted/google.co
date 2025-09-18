use crate::index::SearchHit;
use crate::indexing;
use crate::link::{domain_trust_from_cname_depth, AuthorityStore};
use crate::proto::http_like::{Request, Response};
use crate::query::parse_query;
use crate::search::{merge_topk, normalize_key, HotQueryCache};
use crate::services;
use anyhow::Result;
use gurt_api::response::{SearchResponse, SearchResultItem};
use gurt_api::status::StatusCode;
use serde_json;

#[cfg(feature = "ext-web")]
use gurt_macros as _; // ensure macro crate is linked when feature is enabled
#[cfg(feature = "ext-web")]
use gurt_web;
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
#[cfg(feature = "ext-web")]
use std::sync::OnceLock;

static HOT_CACHE: Lazy<HotQueryCache> =
    Lazy::new(|| HotQueryCache::new(std::time::Duration::from_secs(20)));
static AUTH_STORE: Lazy<std::sync::Mutex<AuthorityStore>> =
    Lazy::new(|| std::sync::Mutex::new(AuthorityStore::new()));

pub fn handle(req: Request) -> Result<Response> {
    handle_with_peer(req, None)
}

fn handle_search(req: Request) -> Result<Response> {
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
    let engine = services::index_engine();
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

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .to_string()
}

fn json_response(code: StatusCode, body: Vec<u8>) -> Response {
    if code == StatusCode::Ok && std::env::var("GURT_DEBUG_RESULTS").ok().filter(|v| v != "0").is_some() {
        if let Ok(txt) = std::str::from_utf8(&body) {
            eprintln!("[results] {}", txt);
        }
    }
    Response {
        code,
        headers: vec![("content-type".into(), "application/json".into())],
        body,
    }
}

fn html_response(code: StatusCode, body: Vec<u8>) -> Response {
    Response {
        code,
        headers: vec![("content-type".into(), "text/html".into())],
        body,
    }
}

fn ui_dir() -> std::path::PathBuf {
    // Allow override for tests or deployments
    if let Ok(dir) = std::env::var("GURT_UI_DIR") {
        return std::path::PathBuf::from(dir);
    }
    // Default to crate's ui/ directory using compile-time manifest dir
    let base: &str = env!("CARGO_MANIFEST_DIR");
    let mut p = std::path::PathBuf::from(base);
    p.push("ui");
    p
}

fn serve_index_html() -> Result<Response> {
    let mut p = ui_dir();
    p.push("index.html");
    match std::fs::read(&p) {
        Ok(bytes) => Ok(html_response(StatusCode::Ok, bytes)),
        Err(_) => Ok(html_response(
            StatusCode::Ok,
            DEFAULT_INDEX_HTML.as_bytes().to_vec(),
        )),
    }
}

fn serve_search_html() -> Result<Response> {
    let mut p = ui_dir();
    p.push("search.html");
    match std::fs::read(&p) {
        Ok(bytes) => Ok(html_response(StatusCode::Ok, bytes)),
        Err(_) => Ok(html_response(
            StatusCode::Ok,
            DEFAULT_SEARCH_HTML.as_bytes().to_vec(),
        )),
    }
}

fn escape_html(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            _ => c.to_string(),
        })
        .collect::<String>()
}

fn render_search_ssr(q: &str) -> Result<Response> {
    let pq = parse_query(q);
    let page = 1usize;
    let size = 10usize;
    let engine = services::index_engine();
    let hits = engine.search(&pq, page, size).unwrap_or_default();
    let results = rescore_and_convert(hits, size as usize);

    let mut items = String::new();
    for r in &results {
        let title = if r.title.trim().is_empty() { r.url.clone() } else { r.title.clone() };
        let url = escape_html(&r.url);
        let etitle = escape_html(&title);
        items.push_str(&format!(
            "<li style=\"w-full rounded border border-[#202637] bg-[#0f1526] hover:bg-[#111a2e] p-3\">\
                <a href=\"{url}\" style=\"text-[#e6e6f0] hover:text-[#6366f1] font-bold\">{etitle}</a>\
                <div style=\"text-sm text-[#9ca3af] mt-1\">{url}</div>\
            </li>"
        ));
    }

        let body = format!(
                "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"/><title>Results - {q}</title></head>\
                 <body style=\"bg-[#0b0f19] text-[#e6e6f0] font-sans\">\
                     <div style=\"max-w-[900px] mx-auto p-6 flex flex-col gap-4\">\
                         <header style=\"flex items-center justify-between\">\
                             <h1 style=\"text-3xl text-[#6366f1] font-bold\">Results</h1>\
                             <nav style=\"flex gap-3 text-sm text-[#9ca3af]\">\
                                 <a href=\"/\" style=\"text-[#e6e6f0] hover:text-[#6366f1]\">Home</a>\
                                 <span>|</span>\
                                 <a href=\"/domains\" style=\"text-[#e6e6f0] hover:text-[#6366f1]\">Submit domain</a>\
                             </nav>\
                         </header>\
                         <form id=\"qform\" style=\"flex items-center gap-2\" action=\"/search\" method=\"GET\">\
                             <input id=\"q\" name=\"q\" type=\"text\" value=\"{}\" placeholder=\"Search...\" autofocus autocomplete=\"off\" style=\"flex-1 min-w-0 p-2 bg-[#0f1526] text-[#e6e6f0] rounded border border-[#202637] focus:ring-2 ring-[#4f46e5]\"/>\
                             <button type=\"submit\" style=\"bg-[#6366f1] text-white rounded px-4 py-2 hover:bg-[#4f46e5] active:bg-[#4338ca]\">Search</button>\
                         </form>\
                                     <ul id=\"results\" style=\"mt-4 flex flex-col gap-2 items-stretch w-full list-none m-0 p-0\">{}</ul>\
                         <script type=\"text/lua\" src=\"/assets/utils.lua\"></script>\
                         <script type=\"text/lua\" src=\"/assets/app.lua\"></script>\
                     </div>\
                 </body></html>",
                escape_html(q),
                items
        );
    Ok(html_response(StatusCode::Ok, body.into_bytes()))
}

fn serve_domains_html() -> Result<Response> {
    let mut p = ui_dir();
    p.push("domains.html");
    match std::fs::read(&p) {
        Ok(bytes) => Ok(html_response(StatusCode::Ok, bytes)),
        Err(_) => Ok(html_response(
            StatusCode::Ok,
            DEFAULT_DOMAINS_HTML.as_bytes().to_vec(),
        )),
    }
}

fn serve_asset(path: &str) -> Result<Response> {
    let rel = &path["/assets/".len()..];
    if rel.contains("..") {
        // block traversal
        return Ok(Response {
            code: StatusCode::BadRequest,
            headers: vec![],
            body: vec![],
        });
    }
    let mut p = ui_dir();
    p.push("assets");
    p.push(rel);
    match std::fs::read(&p) {
        Ok(bytes) => Ok(Response {
            code: StatusCode::Ok,
            headers: vec![("content-type".into(), content_type_for(&p))],
            body: bytes,
        }),
        Err(_) => Ok(Response {
            code: StatusCode::BadRequest,
            headers: vec![],
            body: vec![],
        }),
    }
}

fn content_type_for(p: &std::path::Path) -> String {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html".into(),
        "css" => "text/css".into(),
        "js" => "application/javascript".into(),
        "json" => "application/json".into(),
        "lua" => "text/lua".into(),
        "png" => "image/png".into(),
        "jpg" | "jpeg" => "image/jpeg".into(),
        "svg" => "image/svg+xml".into(),
        _ => "application/octet-stream".into(),
    }
}

// Fallback inline UI if disk files are missing
static DEFAULT_INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\" />
  <title>Gurted Search</title>
  <style>
    body { bg-[#0b0f19] text-[#e6e6f0] font-sans }
    a { text-[#e6e6f0] hover:text-[#6366f1] }
    ul { list-none }
  </style>
</head>
<body style=\"bg-[#0b0f19] text-[#e6e6f0]\">
  <div style=\"max-w-[800px] mx-auto p-4 flex flex-col gap-4\">
    <h1 style=\"text-3xl text-[#6366f1] font-bold\">Gurted Search</h1>
    <form id=\"qform\" style=\"flex gap-2\">
      <input id=\"q\" type=\"text\" placeholder=\"Search...\" autofocus style=\"flex w-full p-2 bg-[#0f1526] text-[#e6e6f0] rounded border border-[#202637]\" />
      <button type=\"submit\" style=\"bg-[#6366f1] text-white rounded px-4 py-2 hover:bg-[#4f46e5]\">Search</button>
    </form>
  </div>
  <script type=\"text/lua\" src=\"/assets/app.lua\"></script>
  </body>
</html>
"#;

// Default search page if disk file missing
static DEFAULT_SEARCH_HTML: &str = r#"<!DOCTYPE html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\" />
  <title>Gurted Search - Results</title>
  <style>
    body { bg-[#0b0f19] text-[#e6e6f0] font-sans }
    a { text-[#e6e6f0] hover:text-[#6366f1] }
    ul { list-none }
  </style>
</head>
<body style=\"bg-[#0b0f19] text-[#e6e6f0]\">
  <div style=\"max-w-[800px] mx-auto p-4 flex flex-col gap-4\">
    <h1 style=\"text-3xl text-[#6366f1] font-bold\">Results</h1>
    <form id=\"qform\" style=\"flex gap-2\">
      <input id=\"q\" type=\"text\" placeholder=\"Search...\" autofocus style=\"flex w-full p-2 bg-[#0f1526] text-[#e6e6f0] rounded border border-[#202637]\" />
      <button type=\"submit\" style=\"bg-[#6366f1] text-white rounded px-4 py-2 hover:bg-[#4f46e5]\">Search</button>
    </form>
    <ul id=\"results\" style=\"mt-4 flex flex-col gap-2\"></ul>
  </div>
  <script type=\"text/lua\" src=\"/assets/app.lua\"></script>
  </body>
</html>
"#;

static DEFAULT_DOMAINS_HTML: &str = r#"<!DOCTYPE html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\" />
  <title>Submit Domain - Gurted Search</title>
  <style>
    body { bg-[#0b0f19] text-[#e6e6f0] font-sans }
    a { text-[#e6e6f0] hover:text-[#6366f1] }
    input { bg-[#0f1526] text-[#e6e6f0] }
  </style>
</head>
<body style=\"bg-[#0b0f19] text-[#e6e6f0]\">
  <div style=\"max-w-[720px] mx-auto p-4 flex flex-col gap-4\">
    <h1 style=\"text-3xl text-[#6366f1] font-bold\">Submit a Domain</h1>
    <p>Share a gurt:// domain to enqueue it for indexing.</p>
    <form id=\"domain-form\" style=\"flex gap-2 items-center\">
      <input id=\"domain\" type=\"text\" placeholder=\"example.real\" autocomplete=\"off\" style=\"flex-1 min-w-0 p-2 rounded border border-[#202637] focus:ring-2 ring-[#4f46e5]\" />
      <button type=\"submit\" style=\"bg-[#6366f1] text-white rounded px-4 py-2 hover:bg-[#4f46e5]\">Submit</button>
    </form>
    <div id=\"status\" style=\"min-h-[24px]\"></div>
    <a href=\"/\" style=\"text-sm text-[#9ca3af]\">Back to search</a>
  </div>
  <script type=\"text/lua\" src=\"/assets/utils.lua\"></script>
  <script type=\"text/lua\" src=\"/assets/domains.lua\"></script>
</body>
</html>
"#;

fn rescore_and_convert(hits: Vec<SearchHit>, k: usize) -> Vec<SearchResultItem> {
    if hits.is_empty() {
        return Vec::new();
    }
    let max_bm = hits
        .iter()
        .map(|h| h.score)
        .fold(0.0f32, |a, b| a.max(b))
        .max(1e-6);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let half_life_secs = 7 * 24 * 3600i64; // 7 days
    let weights = (0.6f64, 0.2f64, 0.1f64, 0.1f64); // (bm25, authority, trust, recency)
    // Note: these are hand-wavy and will move as we get real data.
    let store = AUTH_STORE.lock().unwrap();
    let mut rescored: Vec<SearchResultItem> = hits
        .into_iter()
        .map(|h| {
            let bm25 = (h.score / max_bm) as f64;
            let auth = store.get(&h.url).unwrap_or(0.0) as f64;
            let trust = domain_trust_from_cname_depth(0); // TODO: plug in real CNAME depth when the DNS bits land
            let age = (now - h.fetch_time).max(0) as f64;
            let recency = if half_life_secs > 0 {
                (0.5f64).powf(age / (half_life_secs as f64))
            } else {
                0.0
            };
            let score =
                weights.0 * bm25 + weights.1 * auth + weights.2 * trust + weights.3 * recency;
            SearchResultItem {
                title: h.title,
                url: h.url,
                score: score as f32,
            }
        })
        .collect();
    // Merge top-k (single shard here) and sort by score desc
    rescored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let merged = merge_topk(vec![rescored], k);
    merged
}

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

#[cfg(not(feature = "ext-web"))]
pub fn handle_with_peer(req: Request, peer: Option<SocketAddr>) -> Result<Response> {
    match (
        req.method.as_str(),
        req.path.split('?').next().unwrap_or(""),
    ) {
        ("GET", "/") => serve_index_html(),
        ("GET", "/search") => {
            // SSR: if q is present render server-side results, else serve template
            if let Some(query) = req.query() {
                for pair in query.split('&') {
                    if let Some((k,v)) = pair.split_once('=') {
                        if k == "q" {
                            return render_search_ssr(&percent_decode(v));
                        }
                    }
                }
            }
            serve_search_html()
        },
        ("GET", "/domains") => serve_domains_html(),
        ("GET", path) if path.starts_with("/assets/") => serve_asset(path),
        ("GET", "/health/ready") => Ok(json_response(
            StatusCode::Ok,
            b"{\"status\":\"ready\"}".to_vec(),
        )),
        ("GET", "/api/search") => handle_search(req),
        ("POST", "/api/sites") => handle_add_site(req, peer),
        _ => Ok(Response {
            code: StatusCode::BadRequest,
            headers: vec![],
            body: vec![],
        }),
    }
}

#[cfg(feature = "ext-web")]
pub fn handle_with_peer(req: Request, peer: Option<SocketAddr>) -> Result<Response> {
    register_routes_once();
    let method = req.method.as_str();
    let path = req.path.split('?').next().unwrap_or("");
    if gurt_web::is_registered(method, path) {
        return dispatch(req, peer);
    }
    match (method, path) {
        ("GET", p) if p.starts_with("/assets/") => serve_asset(p),
        _ => Ok(Response {
            code: StatusCode::BadRequest,
            headers: vec![],
            body: vec![],
        }),
    }
}

#[cfg(feature = "ext-web")]
fn dispatch(req: Request, peer: Option<SocketAddr>) -> Result<Response> {
    match (
        req.method.as_str(),
        req.path.split('?').next().unwrap_or(""),
    ) {
        ("GET", "/") => web_root(req, peer),
        ("GET", "/search") => web_search(req, peer),
        ("GET", "/domains") => web_domains(req, peer),
        ("GET", "/health/ready") => web_health_ready(req, peer),
        ("GET", "/api/search") => web_api_search(req, peer),
        ("POST", "/api/sites") => web_api_sites(req, peer),
        _ => Ok(Response {
            code: StatusCode::BadRequest,
            headers: vec![],
            body: vec![],
        }),
    }
}

#[cfg(feature = "ext-web")]
fn register_routes_once() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        web_root__register();
        web_search__register();
        web_domains__register();
        web_health_ready__register();
        web_api_search__register();
        web_api_sites__register();
    });
}

#[cfg(feature = "ext-web")]
#[gurt_macros::route(method = "GET", path = "/")]
fn web_root(_req: Request, _peer: Option<SocketAddr>) -> Result<Response> {
    serve_index_html()
}

#[cfg(feature = "ext-web")]
#[gurt_macros::route(method = "GET", path = "/search")]
fn web_search(req: Request, _peer: Option<SocketAddr>) -> Result<Response> {
    if let Some(query) = req.query() {
        for pair in query.split('&') {
            if let Some((k,v)) = pair.split_once('=') {
                if k == "q" { return render_search_ssr(&percent_decode(v)); }
            }
        }
    }
    serve_search_html()
}

#[cfg(feature = "ext-web")]
#[gurt_macros::route(method = "GET", path = "/domains")]
fn web_domains(_req: Request, _peer: Option<SocketAddr>) -> Result<Response> {
    serve_domains_html()
}

#[cfg(feature = "ext-web")]
#[gurt_macros::route(method = "GET", path = "/health/ready")]
fn web_health_ready(_req: Request, _peer: Option<SocketAddr>) -> Result<Response> {
    Ok(json_response(
        StatusCode::Ok,
        b"{\"status\":\"ready\"}".to_vec(),
    ))
}

#[cfg(feature = "ext-web")]
#[gurt_macros::route(method = "GET", path = "/api/search")]
fn web_api_search(req: Request, _peer: Option<SocketAddr>) -> Result<Response> {
    handle_search(req)
}

#[cfg(feature = "ext-web")]
#[gurt_macros::route(method = "POST", path = "/api/sites")]
fn web_api_sites(req: Request, peer: Option<SocketAddr>) -> Result<Response> {
    handle_add_site(req, peer)
}

fn handle_add_site(req: Request, peer: Option<SocketAddr>) -> Result<Response> {
    // Determine client IP (peer preferred, fallback to x-forwarded-for)
    let ip_from_peer = peer.map(|p| p.ip());
    let ip_from_header = get_header(&req, "x-forwarded-for")
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

    indexing::enqueue_domain(domain.clone());

    let body = serde_json::to_vec(&serde_json::json!({
        "status": "accepted",
        "domain": domain
    }))
    .unwrap_or_else(|_| b"{}".to_vec());
    Ok(json_response(StatusCode::Ok, body))
}

fn get_header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    let lname = name.to_ascii_lowercase();
    for (k, v) in &req.headers {
        if k.eq_ignore_ascii_case(&lname) {
            return Some(v.as_str());
        }
    }
    None
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
