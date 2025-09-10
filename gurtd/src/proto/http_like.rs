use gurt_api::{limits::{enforce_max_message_size, MAX_MESSAGE_BYTES}, status::StatusCode};
use memchr::{memmem::Finder, memchr};
use anyhow::Result;
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn query(&self) -> Option<&str> {
        self.path.split_once('?').map(|(_, q)| q)
    }
}

pub async fn read_request<S>(stream: &mut S) -> Result<Request, StatusCode>
where
    S: AsyncReadExt + Unpin,
{
    // Read headers up to CRLFCRLF with total cap
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 2048];
    // Track where to resume scanning for CRLFCRLF to avoid O(n^2) rescans
    let mut search_from: usize = 0;
    let finder = Finder::new(b"\r\n\r\n");
    let mut header_end: Option<usize> = None;
    loop {
        let n = stream.read(&mut tmp).await.map_err(|_| StatusCode::InternalServerError)?;
        if n == 0 { return Err(StatusCode::BadRequest); }
        let before_len = buf.len();
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > MAX_MESSAGE_BYTES { return Err(StatusCode::RequestEntityTooLarge); }
        // Only scan newly appended region (with overlap for boundary cases)
        let start = search_from.saturating_sub(3);
        if let Some(rel) = finder.find(&buf[start..]) {
            header_end = Some(start + rel);
            break;
        }
        search_from = before_len + n;
    }
    let header_end = header_end.ok_or(StatusCode::BadRequest)?;
    let (head, rest) = buf.split_at(header_end + 4);
    let head_str = std::str::from_utf8(head).map_err(|_| StatusCode::BadRequest)?;
    let mut lines = head_str.split("\r\n");
    let start = lines.next().unwrap_or("");
    let mut sp = start.split_whitespace();
    let method = sp.next().unwrap_or("").to_string();
    let path = sp.next().unwrap_or("").to_string();
    let _version = sp.next().unwrap_or("");
    if method.is_empty() || path.is_empty() { return Err(StatusCode::BadRequest); }

    let mut headers = Vec::new();
    let mut content_length: usize = 0;
    for line in lines {
        if line.is_empty() { continue; }
        if let Some(idx) = memchr(b':', line.as_bytes()) {
            let (name_raw, value_raw) = line.split_at(idx);
            let name = name_raw.trim().to_ascii_lowercase();
            let value = value_raw[1..].trim().to_string(); // skip ':'
            if name == "content-length" {
                if let Ok(n) = value.parse::<usize>() { content_length = n; }
            }
            headers.push((name, value));
        }
    }

    // Read body if present
    let mut body = Vec::new();
    if content_length > 0 {
        if header_end + 4 + content_length > MAX_MESSAGE_BYTES { return Err(StatusCode::RequestEntityTooLarge); }
        if !rest.is_empty() {
            body.extend_from_slice(&rest);
        }
        while body.len() < content_length {
            let mut chunk = [0u8; 4096];
            let n = stream.read(&mut chunk).await.map_err(|_| StatusCode::InternalServerError)?;
            if n == 0 { break; }
            body.extend_from_slice(&chunk[..n]);
            enforce_max_message_size(header_end + 4 + body.len()).map_err(|_| StatusCode::RequestEntityTooLarge)?;
        }
        body.truncate(content_length);
    }

    Ok(Request { method, path, headers, body })
}

// kept no helper; detection is handled incrementally with memchr::memmem

pub struct Response {
    pub code: StatusCode,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn into_bytes(self) -> Vec<u8> {
        make_response(self.code, &self.headers, &self.body)
    }
}

pub fn make_response(code: StatusCode, headers: &[(String, String)], body: &[u8]) -> Vec<u8> {
    let reason = match code {
        StatusCode::Ok => "OK",
        StatusCode::BadRequest => "BAD_REQUEST",
        StatusCode::RequestEntityTooLarge => "TOO_LARGE",
        StatusCode::InternalServerError => "INTERNAL_SERVER_ERROR",
    };
    let date = httpdate::fmt_http_date(std::time::SystemTime::now());
    let mut out = format!(
        "GURT/1.0.0 {} {}\r\nserver: GURT/1.0.0\r\ndate: {}\r\n",
        code.as_u16(), reason, date
    ).into_bytes();
    let mut had_ct = false;
    let mut had_cl = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-type") { had_ct = true; }
        if k.eq_ignore_ascii_case("content-length") { had_cl = true; }
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(v.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    if !had_ct { out.extend_from_slice(b"content-type: application/json\r\n"); }
    if !had_cl { out.extend_from_slice(format!("content-length: {}\r\n", body.len()).as_bytes()); }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

pub fn make_empty_response(code: StatusCode) -> String {
    let bytes = make_response(code, &[], &[]);
    String::from_utf8(bytes).unwrap_or_default()
}
