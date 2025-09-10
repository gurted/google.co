use anyhow::Result;
use gurt_api::status::StatusCode;
use gurt_api::response::{SearchResponse, SearchResultItem};
use serde_json;
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
    // Overload and internal error mapping (stubbed via env flags for now)
    if std::env::var("GURT_OVERLOADED").ok().filter(|v| v != "0").is_some() {
        return Ok(Response { code: StatusCode::TooManyRequests, headers: vec![], body: vec![] });
    }
    if std::env::var("GURT_FORCE_500").ok().filter(|v| v != "0").is_some() {
        return Ok(Response { code: StatusCode::InternalServerError, headers: vec![], body: vec![] });
    }
    let resp = SearchResponse {
        query: q,
        total: 0,
        page: 1,
        size: 10,
        results: Vec::new(),
    };
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
