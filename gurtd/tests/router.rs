use gurtd::proto::http_like::Request;
use once_cell::sync::Lazy;
use std::sync::Mutex;

static TEST_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
use serde_json::Value;
use gurtd::router::handle;

fn make_get(path: &str) -> Request {
    Request {
        method: "GET".into(),
        path: path.into(),
        headers: vec![],
        body: vec![],
    }
}

#[test]
fn health_ready_returns_200_and_json() {
    let _g = TEST_MUTEX.lock().unwrap();
    let req = make_get("/health/ready");
    let resp = handle(req).expect("router should handle");
    assert_eq!(resp.code.as_u16(), 200);
    let ct = resp.headers.iter().find(|(k, _)| k == "content-type").map(|(_, v)| v.as_str()).unwrap_or("");
    assert_eq!(ct, "application/json");
    assert_eq!(String::from_utf8_lossy(&resp.body), "{\"status\":\"ready\"}");
}

#[test]
fn search_with_empty_q_returns_400() {
    let _g = TEST_MUTEX.lock().unwrap();
    let req = make_get("/api/search?q=");
    let resp = handle(req).expect("router should handle");
    assert_eq!(resp.code.as_u16(), 400);
}

#[test]
fn search_with_query_returns_200_and_placeholder_json() {
    let _g = TEST_MUTEX.lock().unwrap();
    let req = make_get("/api/search?q=rust");
    let resp = handle(req).expect("router should handle");
    assert_eq!(resp.code.as_u16(), 200);
    let v: Value = serde_json::from_slice(&resp.body).expect("valid json");
    // Schema keys
    assert_eq!(v["query"], "rust");
    assert_eq!(v["total"], 0);
    assert_eq!(v["page"], 1);
    assert_eq!(v["size"], 10);
    assert!(v["results"].is_array());
}

#[test]
fn search_returns_429_when_overloaded() {
    let _g = TEST_MUTEX.lock().unwrap();
    std::env::set_var("GURT_OVERLOADED", "1");
    let req = make_get("/api/search?q=rust");
    let resp = handle(req).expect("router should handle");
    assert_eq!(resp.code.as_u16(), 429);
    std::env::remove_var("GURT_OVERLOADED");
}

#[test]
fn search_returns_500_on_internal_error() {
    let _g = TEST_MUTEX.lock().unwrap();
    std::env::set_var("GURT_FORCE_500", "1");
    let req = make_get("/api/search?q=rust");
    let resp = handle(req).expect("router should handle");
    assert_eq!(resp.code.as_u16(), 500);
    std::env::remove_var("GURT_FORCE_500");
}

// New tests for POST /api/sites with IP rate limiting
use serde_json::json;

fn make_post_json(path: &str, ip: &str, body: serde_json::Value) -> Request {
    let bytes = serde_json::to_vec(&body).expect("json");
    Request {
        method: "POST".into(),
        path: path.into(),
        headers: vec![
            ("content-type".into(), "application/json".into()),
            ("content-length".into(), bytes.len().to_string()),
            ("x-forwarded-for".into(), ip.into()),
        ],
        body: bytes,
    }
}

#[test]
fn add_site_accepts_valid_domain() {
    let _g = TEST_MUTEX.lock().unwrap();
    let req = make_post_json("/api/sites", "10.0.0.1", json!({"domain":"example.real"}));
    let resp = handle(req).expect("router should handle");
    assert_eq!(resp.code.as_u16(), 200);
    let ct = resp.headers.iter().find(|(k, _)| k == "content-type").map(|(_, v)| v.as_str()).unwrap_or("");
    assert_eq!(ct, "application/json");
    let v: Value = serde_json::from_slice(&resp.body).expect("valid json");
    assert_eq!(v["status"], "accepted");
    assert_eq!(v["domain"], "example.real");
}

#[test]
fn add_site_rate_limited_by_ip() {
    let _g = TEST_MUTEX.lock().unwrap();
    let ip = "10.0.0.2";
    // Allow up to default 5 in window, expect 429 on 6th
    for i in 0..5 {
        let req = make_post_json("/api/sites", ip, json!({"domain": format!("site{i}.real")}));
        let resp = handle(req).expect("router ok");
        assert_eq!(resp.code.as_u16(), 200, "attempt {} should be 200", i+1);
    }
    let req = make_post_json("/api/sites", ip, json!({"domain":"overflow.real"}));
    let resp = handle(req).expect("router ok");
    assert_eq!(resp.code.as_u16(), 429);
}

#[test]
fn add_site_rejects_bad_payload() {
    let _g = TEST_MUTEX.lock().unwrap();
    let req = make_post_json("/api/sites", "10.0.0.3", json!({"domain": ""}));
    let resp = handle(req).expect("router should handle");
    assert_eq!(resp.code.as_u16(), 400);
}
