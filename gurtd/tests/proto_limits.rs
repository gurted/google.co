use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
use gurt_api::limits::MAX_MESSAGE_BYTES;
use gurtd::proto::http_like::{read_request, make_empty_response};

// Integration-style test at the server protocol layer: feed >10MB request
// and assert that the emitted response is a 413 TOO_LARGE frame.
#[tokio::test]
async fn oversized_request_emits_413_response() {
    // Duplex capacity is small; the server task will drain as we write.
    let (mut client, mut server) = duplex(8192);

    // Spawn a server task that attempts to read a request, then writes
    // an empty error response if the request is rejected (mirrors gurtd main).
    let srv = tokio::spawn(async move {
        match read_request(&mut server).await {
            Ok(_req) => {
                // Unexpected in this test; write OK for visibility
                let ok = make_empty_response(gurt_api::status::StatusCode::Ok);
                let _ = server.write_all(ok.as_bytes()).await;
            }
            Err(code) => {
                let resp = make_empty_response(code);
                let _ = server.write_all(resp.as_bytes()).await;
            }
        }
    });

    // Build a request that exceeds MAX_MESSAGE_BYTES before CRLFCRLF is seen
    // to trigger 413 during header accumulation.
    let mut req = Vec::with_capacity(MAX_MESSAGE_BYTES + 1024);
    req.extend_from_slice(b"GET /search HTTP/1.1\r\n");
    // Large header line without terminating CRLFCRLF until after we exceed the cap
    req.extend_from_slice(b"x-fill: ");
    req.extend(std::iter::repeat(b'a').take(MAX_MESSAGE_BYTES + 1));
    req.extend_from_slice(b"\r\n\r\n");

    // Write the oversized request and then read the server's response
    client.write_all(&req).await.unwrap();
    client.flush().await.unwrap();

    // Read response
    let mut buf = Vec::new();
    buf.resize(1024, 0);
    let n = client.read(&mut buf).await.unwrap();
    let resp = String::from_utf8_lossy(&buf[..n]);
    assert!(resp.starts_with("GURT/1.0.0 413 TOO_LARGE"), "response was: {}", resp);

    srv.await.unwrap();
}

