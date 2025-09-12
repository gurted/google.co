use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use gurtd::crawler::client::{ClientError, DynStream, GurtClient};
use gurtd::crawler::sitemap::{fetch_sitemap_urls, parse_sitemap_xml};

#[tokio::test]
async fn sitemap_fetch_and_parse_urls() {
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

    let fut = fetch_sitemap_urls(&client, "example.real");

    let srv = async move {
        // handshake
        server.write_all(b"GURT/1.0.0 101 SWITCHING_PROTOCOLS\r\n\r\n").await.unwrap();
        // read client request
        let mut _buf = [0u8; 128];
        let _ = server.read(&mut _buf).await.unwrap_or(0);
        // sitemap body
        let body = br#"<?xml version=\"1.0\"?>
<urlset>
  <url><loc>gurt://example.real/</loc></url>
  <url><loc>gurt://example.real/docs</loc></url>
</urlset>"#;
        let head = format!("GURT/1.0.0 200 OK\r\ncontent-length: {}\r\ncontent-type: application/xml\r\n\r\n", body.len());
        server.write_all(head.as_bytes()).await.unwrap();
        server.write_all(body).await.unwrap();
    };

    let (urls, _) = tokio::join!(fut, srv);
    assert_eq!(urls, vec![
        "gurt://example.real/".to_string(),
        "gurt://example.real/docs".to_string(),
    ]);
}

#[test]
fn sitemap_parse_empty_when_no_loc() {
    let xml = "<urlset><url><lastmod>today</lastmod></url></urlset>";
    let urls = parse_sitemap_xml(xml);
    assert!(urls.is_empty());
}

