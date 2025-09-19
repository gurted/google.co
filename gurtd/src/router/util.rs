use gurt_api::status::StatusCode;

use crate::proto::http_like::Response;

pub fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s).decode_utf8_lossy().to_string()
}

pub fn json_response(code: StatusCode, body: Vec<u8>) -> Response {
    if code == StatusCode::Ok
        && std::env::var("GURT_DEBUG_RESULTS").ok().filter(|v| v != "0").is_some()
    {
        if let Ok(txt) = std::str::from_utf8(&body) {
            eprintln!("[results] {}", txt);
        }
    }
    Response { code, headers: vec![("content-type".into(), "application/json".into())], body }
}

pub fn get_header<'a>(req: &'a crate::proto::http_like::Request, name: &str) -> Option<&'a str> {
    let lname = name.to_ascii_lowercase();
    for (k, v) in &req.headers {
        if k.eq_ignore_ascii_case(&lname) { return Some(v.as_str()); }
    }
    None
}

pub fn escape_html(s: &str) -> String {
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
