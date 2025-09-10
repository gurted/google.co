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
    let req = make_get("/search?q=");
    let resp = handle(req).expect("router should handle");
    assert_eq!(resp.code.as_u16(), 400);
}

#[test]
fn search_with_query_returns_200_and_placeholder_json() {
    let _g = TEST_MUTEX.lock().unwrap();
    let req = make_get("/search?q=rust");
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
    let req = make_get("/search?q=rust");
    let resp = handle(req).expect("router should handle");
    assert_eq!(resp.code.as_u16(), 429);
    std::env::remove_var("GURT_OVERLOADED");
}

#[test]
fn search_returns_500_on_internal_error() {
    let _g = TEST_MUTEX.lock().unwrap();
    std::env::set_var("GURT_FORCE_500", "1");
    let req = make_get("/search?q=rust");
    let resp = handle(req).expect("router should handle");
    assert_eq!(resp.code.as_u16(), 500);
    std::env::remove_var("GURT_FORCE_500");
}
