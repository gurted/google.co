use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use gurtd::crawler::client::{ClientError, DynStream, GurtClient};
use gurtd::crawler::robots::{RobotsTxt, is_allowed_with_robots};

#[tokio::test]
async fn robots_fetch_and_allow_deny() {
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

    let fut = RobotsTxt::fetch_for_domain(&client, "example.real");

    // Simulate handshake and robots response
    let srv = async move {
        // Handshake
        server.write_all(b"GURT/1.0.0 101 SWITCHING_PROTOCOLS\r\n\r\n").await.unwrap();
        // Read client's request
        let mut _buf = [0u8; 256];
        let _ = server.read(&mut _buf).await.unwrap_or(0);
        // robots.txt body
        let body = b"User-agent: *\nDisallow: /private\nAllow: /private/open\nCrawl-delay: 2\n";
        let head = format!("GURT/1.0.0 200 OK\r\ncontent-length: {}\r\ncontent-type: text/plain\r\n\r\n", body.len());
        server.write_all(head.as_bytes()).await.unwrap();
        server.write_all(body).await.unwrap();
    };

    let (res, _) = tokio::join!(fut, srv);
    let robots = res.expect("robots fetched");
    assert!(!robots.is_allowed("gurtbot", "/private/secret"));
    assert!(robots.is_allowed("gurtbot", "/private/open/index"));
    assert_eq!(robots.crawl_delay("gurtbot").unwrap().as_secs(), 2);
}

#[tokio::test]
async fn robots_absent_defaults_to_allow() {
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

    let fut = RobotsTxt::fetch_for_domain(&client, "example.real");

    let srv = async move {
        // handshake
        server.write_all(b"GURT/1.0.0 101 SWITCHING_PROTOCOLS\r\n\r\n").await.unwrap();
        // read client's request
        let mut _buf = [0u8; 128];
        let _ = server.read(&mut _buf).await.unwrap_or(0);
        // 404 not found robots
        server.write_all(b"GURT/1.0.0 404 NOT_FOUND\r\ncontent-length: 0\r\n\r\n").await.unwrap();
    };

    let (res, _) = tokio::join!(fut, srv);
    assert!(res.is_none(), "missing robots should yield None");
    assert!(is_allowed_with_robots(None, "gurtbot", "/any/path"));
}

