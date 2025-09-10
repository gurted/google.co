use anyhow::Result;
use gurt_api::status::StatusCode;
use crate::proto::http_like::{Request, Response};

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
    // Placeholder JSON; real schema in Task 2
    let body = format!(
        "{{\"query\":{q:?},\"total\":0,\"page\":1,\"size\":10,\"results\":[]}}"
    ).into_bytes();
    Ok(json_response(StatusCode::Ok, body))
}

fn percent_decode(s: &str) -> String {
    // Best-effort, minimal decoding
    url::form_urlencoded::parse(s.as_bytes()).into_owned().collect::<Vec<(String,String)>>().into_iter().map(|(k,v)|{
        if k.is_empty() { v } else { format!("{k}={v}") }
    }).collect::<Vec<_>>().join("&")
}

fn json_response(code: StatusCode, body: Vec<u8>) -> Response {
    Response { code, headers: vec![("content-type".into(), "application/json".into())], body }
}

