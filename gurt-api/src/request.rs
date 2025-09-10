use crate::{limits::enforce_max_message_size, status::StatusCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
}

/// Minimal request parsing that enforces protocol message size limit.
/// Returns StatusCode error (e.g., 413) on violations.
pub fn parse_request(raw: &[u8]) -> Result<Request, StatusCode> {
    if let Err(_e) = enforce_max_message_size(raw.len()) {
        return Err(StatusCode::RequestEntityTooLarge);
    }

    // Extremely minimal HTTP-like start-line parsing for tests only.
    // Expected format: "GET /path HTTP/1.1\r\n..."
    let text = match std::str::from_utf8(raw) {
        Ok(t) => t,
        Err(_) => return Err(StatusCode::BadRequest),
    };
    let line = text.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    if method.is_empty() || path.is_empty() {
        return Err(StatusCode::BadRequest);
    }
    Ok(Request { method: method.to_string(), path: path.to_string() })
}

