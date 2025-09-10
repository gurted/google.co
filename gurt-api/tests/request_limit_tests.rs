use gurt_api::{limits::MAX_MESSAGE_BYTES, request::parse_request, status::StatusCode};

fn make_request(prefix: &str, size: usize) -> Vec<u8> {
    let mut body = vec![b'a'; size.saturating_sub(prefix.len())];
    let mut req = prefix.as_bytes().to_vec();
    req.extend_from_slice(&body);
    req
}

#[test]
fn parse_allows_exact_max_size() {
    let start = "GET /health/ready HTTP/1.1\r\n";
    let data = make_request(start, MAX_MESSAGE_BYTES);
    let r = parse_request(&data).expect("should parse at max size");
    assert_eq!(r.path, "/health/ready");
}

#[test]
fn parse_rejects_over_max_size_with_413() {
    let start = "GET /search?q=ok HTTP/1.1\r\n";
    let data = make_request(start, MAX_MESSAGE_BYTES + 1);
    let err = parse_request(&data).expect_err("should error for oversized message");
    assert_eq!(err, StatusCode::RequestEntityTooLarge);
    assert_eq!(err.as_u16(), 413);
}

