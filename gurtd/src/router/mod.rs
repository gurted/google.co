use anyhow::Result;
use std::net::SocketAddr;

use crate::proto::http_like::{Request, Response};
use gurt_api::status::StatusCode;

mod api;
mod search_utils;
mod ui;
mod util;

pub fn handle(req: Request) -> Result<Response> {
    handle_with_peer(req, None)
}

#[cfg(not(feature = "ext-web"))]
pub fn handle_with_peer(req: Request, peer: Option<SocketAddr>) -> Result<Response> {
    match (
        req.method.as_str(),
        req.path.split('?').next().unwrap_or(""),
    ) {
        ("GET", "/") => ui::serve_index_html(),
        ("GET", "/search") => {
            // SSR: if q is present render server-side results, else serve template
            if let Some(query) = req.query() {
                for pair in query.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        if k == "q" {
                            return ui::render_search_ssr(&util::percent_decode(v));
                        }
                    }
                }
            }
            ui::serve_search_html()
        }
        ("GET", "/domains") => ui::serve_domains_html(),
        ("GET", path) if path.starts_with("/assets/") => ui::serve_asset(path),
        ("GET", "/health/ready") => Ok(util::json_response(
            StatusCode::Ok,
            b"{\"status\":\"ready\"}".to_vec(),
        )),
        ("GET", "/api/search") => api::handle_search(req),
        ("POST", "/api/sites") => api::handle_add_site(req, peer),
        _ => Ok(Response {
            code: StatusCode::BadRequest,
            headers: vec![],
            body: vec![],
        }),
    }
}

#[cfg(feature = "ext-web")]
use gurt_macros as _; // ensure macro crate is linked when feature is enabled
#[cfg(feature = "ext-web")]
use gurt_web;
#[cfg(feature = "ext-web")]
use std::sync::OnceLock;

#[cfg(feature = "ext-web")]
pub fn handle_with_peer(req: Request, peer: Option<SocketAddr>) -> Result<Response> {
    register_routes_once();
    let method = req.method.as_str();
    let path = req.path.split('?').next().unwrap_or("");
    if gurt_web::is_registered(method, path) {
        return dispatch(req, peer);
    }
    match (method, path) {
        ("GET", p) if p.starts_with("/assets/") => ui::serve_asset(p),
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
    ui::serve_index_html()
}

#[cfg(feature = "ext-web")]
#[gurt_macros::route(method = "GET", path = "/search")]
fn web_search(req: Request, _peer: Option<SocketAddr>) -> Result<Response> {
    if let Some(query) = req.query() {
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k == "q" {
                    return ui::render_search_ssr(&util::percent_decode(v));
                }
            }
        }
    }
    ui::serve_search_html()
}

#[cfg(feature = "ext-web")]
#[gurt_macros::route(method = "GET", path = "/domains")]
fn web_domains(_req: Request, _peer: Option<SocketAddr>) -> Result<Response> {
    ui::serve_domains_html()
}

#[cfg(feature = "ext-web")]
#[gurt_macros::route(method = "GET", path = "/health/ready")]
fn web_health_ready(_req: Request, _peer: Option<SocketAddr>) -> Result<Response> {
    Ok(util::json_response(
        StatusCode::Ok,
        b"{\"status\":\"ready\"}".to_vec(),
    ))
}

#[cfg(feature = "ext-web")]
#[gurt_macros::route(method = "GET", path = "/api/search")]
fn web_api_search(req: Request, _peer: Option<SocketAddr>) -> Result<Response> {
    api::handle_search(req)
}

#[cfg(feature = "ext-web")]
#[gurt_macros::route(method = "POST", path = "/api/sites")]
fn web_api_sites(req: Request, peer: Option<SocketAddr>) -> Result<Response> {
    api::handle_add_site(req, peer)
}
