use gurt_api::status::StatusCode;

use crate::proto::http_like::Response;

use super::search_utils::{escape_html, rescore_and_convert};
use crate::query::parse_query;
use crate::services;

pub fn ui_dir() -> std::path::PathBuf {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("GURT_UI_DIR") {
        candidates.push(std::path::PathBuf::from(dir));
    }
    candidates.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui"));
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("ui"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ui"));
        }
    }
    for p in &candidates {
        if p.is_dir() {
            if std::env::var("GURT_DEBUG_UI")
                .ok()
                .filter(|v| v != "0")
                .is_some()
            {
                eprintln!("[ui] using directory: {}", p.display());
            }
            return p.clone();
        }
    }
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui")
}

pub fn serve_index_html() -> anyhow::Result<Response> {
    let mut p = ui_dir();
    p.push("index.html");
    match std::fs::read(&p) {
        Ok(bytes) => Ok(html_response(StatusCode::Ok, bytes)),
        Err(e) => {
            if std::env::var("GURT_DEBUG_UI")
                .ok()
                .filter(|v| v != "0")
                .is_some()
            {
                eprintln!("[ui] index fallback; failed to read {}: {}", p.display(), e);
            }
            Ok(html_response(
                StatusCode::Ok,
                DEFAULT_INDEX_HTML.as_bytes().to_vec(),
            ))
        }
    }
}

pub fn serve_search_html() -> anyhow::Result<Response> {
    let mut p = ui_dir();
    p.push("search.html");
    match std::fs::read(&p) {
        Ok(bytes) => Ok(html_response(StatusCode::Ok, bytes)),
        Err(e) => {
            if std::env::var("GURT_DEBUG_UI")
                .ok()
                .filter(|v| v != "0")
                .is_some()
            {
                eprintln!(
                    "[ui] search fallback; failed to read {}: {}",
                    p.display(),
                    e
                );
            }
            Ok(html_response(
                StatusCode::Ok,
                DEFAULT_SEARCH_HTML.as_bytes().to_vec(),
            ))
        }
    }
}

pub fn serve_domains_html() -> anyhow::Result<Response> {
    let mut p = ui_dir();
    p.push("domains.html");
    match std::fs::read(&p) {
        Ok(bytes) => Ok(html_response(StatusCode::Ok, bytes)),
        Err(e) => {
            if std::env::var("GURT_DEBUG_UI")
                .ok()
                .filter(|v| v != "0")
                .is_some()
            {
                eprintln!(
                    "[ui] domains fallback; failed to read {}: {}",
                    p.display(),
                    e
                );
            }
            Ok(html_response(
                StatusCode::Ok,
                DEFAULT_DOMAINS_HTML.as_bytes().to_vec(),
            ))
        }
    }
}

pub fn serve_asset(path: &str) -> anyhow::Result<Response> {
    let rel = &path["/assets/".len()..];
    if rel.contains("..") {
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
        Err(e) => {
            if std::env::var("GURT_DEBUG_UI")
                .ok()
                .filter(|v| v != "0")
                .is_some()
            {
                eprintln!("[ui] asset missing; failed to read {}: {}", p.display(), e);
            }
            Ok(Response {
                code: StatusCode::BadRequest,
                headers: vec![],
                body: vec![],
            })
        }
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

fn html_response(code: StatusCode, body: Vec<u8>) -> Response {
    Response {
        code,
        headers: vec![("content-type".into(), "text/html".into())],
        body,
    }
}

pub fn render_search_ssr(q: &str) -> anyhow::Result<Response> {
    let pq = parse_query(q);
    let page = 1usize;
    let size = 10usize;
    let engine = services::index_engine();
    let hits = engine.search(&pq, page, size).unwrap_or_default();
    let results = rescore_and_convert(hits, size as usize);

    let mut items = String::new();
    for r in &results {
        let title = if r.title.trim().is_empty() {
            r.url.clone()
        } else {
            r.title.clone()
        };
        let url = escape_html(&r.url);
        let etitle = escape_html(&title);
        items.push_str(&format!(
            "<li style=\"w-full p-3 flex flex-col\">\
                <a href=\"{url}\" style=\"text-[#d9d9d9] hover:text-[#6366f1] font-bold\">{etitle}</a>\
                <div style=\"text-sm text-[#808080] mt-1\">{url}</div>\
            </li>"
        ));
    }

    let sq = super::util::escape_html(q);
    let body = format!(
        "<head><meta charset=\"utf-8\"/>
  <font name=\"playfair\" src=\"https://fonts.gstatic.com/l/font?kit=nuFiD-vYSZviVYUb_rj3ij__anPXPT7KnEkQ2Fo0XcXumgW2Kb6JkDjEdDrmYdycAeI\" /><title>Results - {sq}</title></head>
                
<body style=\"bg-[#1a1a1a] text-[#d9d9d9] font-sans\">
  <div style=\"max-w-[1600px] mx-auto p-8 flex flex-col items-center justify-center gap-16 h-full\">
    <h1 style=\"text-4xl font-bold font-playfair\">google.co</h1>
    <form id=\"qform\" style=\"flex items-center gap-2\">
      <input id=\"q\" name=\"q\" type=\"text\" placeholder=\"Search...\" autofocus autocomplete=\"off\" style=\"w-30 flex-1 min-w-0 p-3 bg-[#303030] text-[#e6e6f0] rounded border border-[#353535]\" />
      <button type=\"submit\" style=\"bg-[#a0a0a0] text-[#1a1a1a] rounded px-5 py-3\">Search</button>
    </form>
    <ul id=\"results\" style=\"mt-4 flex flex-col gap-2 items-stretch w-full list-none m-0 p-0\">{items}</ul>
    <div style=\"inline-flex gap-4 text-xs text-[#808080] mt-40\">
      <a href=\"/domains\" style=\"hover:text-[#6366f1] text-xs text-[#808080]\">Submit a domain</a>
      <span style=\"text-xs text-[#808080]\">•</span>
      <a href=\"/domains\" style=\"hover:text-[#6366f1] text-xs text-[#808080]\">ToS</a>
      <span style=\"text-xs text-[#808080]\">•</span>
      <a href=\"/domains\" style=\"hover:text-[#6366f1] text-xs text-[#808080]\">Help</a>
      <span style=\"text-xs text-[#808080]\">•</span>
      <a href=\"/domains\" style=\"hover:text-[#6366f1] text-xs text-[#808080]\">Docs</a>
      <span style=\"text-xs text-[#808080]\">•</span>
      <a href=\"/domains\" style=\"hover:text-[#6366f1] text-xs text-[#808080]\">Stats</a>
      <span style=\"text-xs text-[#808080]\">•</span>
      <a href=\"/domains\" style=\"hover:text-[#6366f1] text-xs text-[#808080]\">Platform Status</a>
    </div>
  </div>
  <script type=\"text/lua\" src=\"/assets/utils.lua\"></script>
  <script type=\"text/lua\" src=\"/assets/app.lua\"></script>
</body>");
    Ok(html_response(StatusCode::Ok, body.into_bytes()))
}

// Fallback inline UI if disk files are missing
static DEFAULT_INDEX_HTML: &str = r#"<head>
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
"#;

// Default search page if disk file missing
static DEFAULT_SEARCH_HTML: &str = r#"<head>
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
"#;

static DEFAULT_DOMAINS_HTML: &str = r#"<head>
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
"#;
