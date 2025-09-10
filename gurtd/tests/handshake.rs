use gurtd::proto::handshake::read_and_respond_handshake;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn handshake_ok_responds_101() {
    let (mut client, mut server) = duplex(4096);

    // Simulate server task
    let srv = tokio::spawn(async move {
        read_and_respond_handshake(&mut server).await.unwrap();
    });

    // Client sends handshake
    let hs = b"HANDSHAKE / GURT/1.0.0\r\nuser-agent: test\r\n\r\n";
    client.write_all(hs).await.unwrap();

    // Read server response
    let mut buf = [0u8; 1024];
    let n = client.read(&mut buf).await.unwrap();
    let s = String::from_utf8_lossy(&buf[..n]);
    assert!(s.starts_with("GURT/1.0.0 101 SWITCHING_PROTOCOLS"));
    assert!(s.contains("alpn: GURT/1.0"));
    srv.await.unwrap();
}

