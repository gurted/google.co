use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncWriteExt, AsyncReadExt};

use gurtd::crawler::client::{ClientError, DynStream, GurtClient};

#[tokio::test]
async fn client_parses_success_response() {
    let (mut server, client_side) = tokio::io::duplex(1 << 16);
    let shared = Arc::new(Mutex::new(Some(client_side)));
    // connector that returns the client side of the duplex stream once
    let connector: Arc<dyn Fn(&str, u16) -> Pin<Box<dyn Future<Output=Result<DynStream, ClientError>> + Send>> + Send + Sync> = {
        let shared = shared.clone();
        Arc::new(move |_host: &str, _port: u16| {
            let cli = shared.lock().unwrap().take().ok_or(ClientError::Connection);
            Box::pin(async move { cli.map(|s| Box::pin(s) as DynStream) })
        })
    };
    let mut client = GurtClient::new_test(connector);
    // Use tiny header chunk to ensure no over-reading in tests
    client.header_read_chunk = 1;
    let fut = client.fetch_with_retries("gurt://example.real/hello", 0);

    // Simulate server: send handshake then a 200 response
    let srv = async move {
        // Handshake 101
        let hs = b"GURT/1.0.0 101 SWITCHING_PROTOCOLS\r\nserver: GURT/1.0.0\r\n\r\n";
        server.write_all(hs).await.unwrap();
        // Read client's request line to ensure ordering
        let mut _buf = [0u8; 128];
        let _ = server.read(&mut _buf).await.unwrap_or(0);
        // Minimal response
        let body = b"hello world";
        let resp = format!(
            "GURT/1.0.0 200 OK\r\ncontent-length: {}\r\ncontent-type: text/plain\r\n\r\n",
            body.len()
        );
        server.write_all(resp.as_bytes()).await.unwrap();
        server.write_all(body).await.unwrap();
    };

    let (res, _) = tokio::join!(fut, srv);
    let resp = res.expect("client ok");
    assert_eq!(resp.code, 200);
    assert_eq!(String::from_utf8_lossy(&resp.body), "hello world");
}

#[tokio::test]
async fn client_errors_on_oversize_body() {
    let (mut server, client_side) = tokio::io::duplex(1 << 16);
    let shared = Arc::new(Mutex::new(Some(client_side)));
    let connector: Arc<dyn Fn(&str, u16) -> Pin<Box<dyn Future<Output=Result<DynStream, ClientError>> + Send>> + Send + Sync> = {
        let shared = shared.clone();
        Arc::new(move |_host: &str, _port: u16| {
            let cli = shared.lock().unwrap().take().ok_or(ClientError::Connection);
            Box::pin(async move { cli.map(|s| Box::pin(s) as DynStream) })
        })
    };
    let mut client = GurtClient::new_test(connector);
    client.header_read_chunk = 1;
    let fut = client.fetch_with_retries("gurt://example.real/oversize", 0);

    let srv = async move {
        // Handshake 101
        let hs = b"GURT/1.0.0 101 SWITCHING_PROTOCOLS\r\n\r\n";
        server.write_all(hs).await.unwrap();
        // Read client's request to keep ordering
        let mut _buf = [0u8; 64];
        let _ = server.read(&mut _buf).await.unwrap_or(0);
        // Oversize by declaring huge length and sending fewer bytes (still triggers cap check)
        let resp = format!(
            "GURT/1.0.0 200 OK\r\ncontent-length: {}\r\n\r\n",
            gurt_api::limits::MAX_MESSAGE_BYTES + 1
        );
        server.write_all(resp.as_bytes()).await.unwrap();
        // write nothing else
    };

    let (res, _) = tokio::join!(fut, srv);
    match res { Err(ClientError::Io) => {}, other => panic!("expected Io, got {:?}", other) }
}

#[tokio::test]
async fn client_retries_on_timeout() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

    // connector that never produces a stream (simulates a hang until timeout)
    let connector: Arc<dyn Fn(&str, u16) -> Pin<Box<dyn Future<Output=Result<DynStream, ClientError>> + Send>> + Send + Sync> = Arc::new(|_host: &str, _port: u16| {
        ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            // but still return a duplex that never responds
            let (_srv, cli) = tokio::io::duplex(1024);
            Ok(Box::pin(cli) as DynStream)
        })
    });
    let mut client = GurtClient::new_test(connector);
    client.req_timeout = Duration::from_millis(10);
    let res = client.fetch_with_retries("gurt://example.real/hang", 1).await;
    assert_eq!(ATTEMPTS.load(Ordering::Relaxed), 2, "should attempt twice");
    assert!(matches!(res, Err(ClientError::Timeout)) || matches!(res, Err(ClientError::Connection)));
}
