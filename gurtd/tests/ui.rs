use gurtd::proto::http_like::Request;
use gurtd::router::handle;

fn get(path: &str) -> Request {
    Request { method: "GET".into(), path: path.into(), headers: vec![], body: vec![] }
}

#[test]
fn root_serves_html_with_lua() {
    let resp = handle(get("/")) .expect("ok");
    assert_eq!(resp.code.as_u16(), 200);
    let ct = resp.headers.iter().find(|(k, _)| k == "content-type").map(|(_, v)| v.as_str()).unwrap_or("");
    assert_eq!(ct, "text/html");
    let body = String::from_utf8_lossy(&resp.body);
    // references external lua asset
    assert!(body.contains("type=\"text/lua\""));
    assert!(body.contains("/assets/app.lua"));
}

#[test]
fn assets_route_serves_lua_file() {
    let resp = handle(get("/assets/app.lua")).expect("ok");
    assert_eq!(resp.code.as_u16(), 200);
    let ct = resp.headers.iter().find(|(k, _)| k == "content-type").map(|(_, v)| v.as_str()).unwrap_or("");
    assert_eq!(ct, "text/lua");
    let body = String::from_utf8_lossy(&resp.body);
    assert!(body.contains("network.fetch('/api/search?q="));
}

// No external CSS route is required; inline utility styles are embedded.

#[test]
fn search_route_serves_html() {
    let resp = handle(get("/search")).expect("ok");
    assert_eq!(resp.code.as_u16(), 200);
    let ct = resp.headers.iter().find(|(k, _)| k == "content-type").map(|(_, v)| v.as_str()).unwrap_or("");
    assert_eq!(ct, "text/html");
}
