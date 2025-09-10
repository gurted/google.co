mod tls;
mod proto;
mod router;

use anyhow::Result;
use rustls::ProtocolVersion;
use tokio::{net::TcpListener, io::AsyncWriteExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Config via env (avoid extra deps):
    // GURT_CERT, GURT_KEY, GURT_ADDR (default 127.0.0.1:4878)
    let cert_path = std::env::var("GURT_CERT").unwrap_or_else(|_| "gurt-server.crt".to_string());
    let key_path = std::env::var("GURT_KEY").unwrap_or_else(|_| "gurt-server.key".to_string());
    let addr = std::env::var("GURT_ADDR").unwrap_or_else(|_| "127.0.0.1:4878".to_string());

    let tls = tls::TlsConfig::load(&cert_path, &key_path)?;
    let acceptor = tls.into_acceptor();

    let listener = TcpListener::bind(&addr).await?;
    eprintln!("gurtd listening on gurt://{}", addr);

    loop {
        let (stream, peer) = listener.accept().await?;
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_conn(stream, acceptor).await {
                eprintln!("connection {} error: {err}", peer);
            }
        });
    }
}

async fn handle_conn(mut tcp: tokio::net::TcpStream, acceptor: tokio_rustls::TlsAcceptor) -> Result<()> {
    // Stage 1: plaintext HANDSHAKE (per docs)
    proto::handshake::read_and_respond_handshake(&mut tcp).await?;

    // Stage 2: upgrade to TLS 1.3 + ALPN GURT/1.0
    let mut tls_stream = acceptor.accept(tcp).await?;
    // Require TLS 1.3 per protocol requirements
    let (_, conn) = tls_stream.get_ref();
    if conn.protocol_version() != Some(ProtocolVersion::TLSv1_3) {
        // Drop connection if not TLS 1.3
        let _ = tls_stream.shutdown().await;
        return Ok(());
    }

    // Stage 3: process a single request (keep-alive/out of scope for now)
    let req = match proto::http_like::read_request(&mut tls_stream).await {
        Ok(r) => r,
        Err(code) => {
            let resp = proto::http_like::make_empty_response(code);
            tls_stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    };

    let response = router::handle(req)?;
    let bytes = response.into_bytes();
    tls_stream.write_all(&bytes).await?;
    Ok(())
}
