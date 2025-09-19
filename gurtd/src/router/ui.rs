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
            if std::env::var("GURT_DEBUG_UI").ok().filter(|v| v != "0").is_some() {
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
            if std::env::var("GURT_DEBUG_UI").ok().filter(|v| v != "0").is_some() {
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
            if std::env::var("GURT_DEBUG_UI").ok().filter(|v| v != "0").is_some() {
                eprintln!("[ui] search fallback; failed to read {}: {}", p.display(), e);
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
            if std::env::var("GURT_DEBUG_UI").ok().filter(|v| v != "0").is_some() {
                eprintln!("[ui] domains fallback; failed to read {}: {}", p.display(), e);
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
            if std::env::var("GURT_DEBUG_UI").ok().filter(|v| v != "0").is_some() {
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
        "<head><meta charset=\"utf-8\"/><title>Results - {q}</title></head>\
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
                 </body>",
        super::util::escape_html(q),
        items
    );
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
