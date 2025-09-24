use gurtd::{proto, router, services, tls};

use anyhow::{Context, Result};
use gurt_db::{Db, DbConfig};
use rustls::ProtocolVersion;
use std::net::SocketAddr;
use tokio::{io::AsyncWriteExt, net::TcpListener};
use dotenv::dotenv;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    // Config via env (avoid extra deps):
    // GURT_CERT, GURT_KEY, GURT_ADDR (default 127.0.0.1:4878)
    let cert_path = std::env::var("GURT_CERT").unwrap_or_else(|_| "gurt-server.crt".to_string());
    let key_path = std::env::var("GURT_KEY").unwrap_or_else(|_| "gurt-server.key".to_string());
    let addr = std::env::var("GURT_ADDR").unwrap_or_else(|_| "127.0.0.1:4878".to_string());

    let pool = {
        let db_cfg = DbConfig::from_env();
        eprintln!(
            "[db] configuration loaded\n  url: {}",
            db_cfg
                .database_url
                .as_deref()
                .map(|_| "<set>")
                .unwrap_or("<missing>")
        );
        let db = Db::new(db_cfg);
        eprintln!("[db] initializing connection pool");
        db.init().await.with_context(|| "database init failed")?;
        db.get_pool()
            .await
            .with_context(|| "database pool acquisition failed")?
            .clone()
    };
    services::init(pool);
    eprintln!("[db] pool ready");

    eprintln!("[tls] loading server certificate and key\n  cert: {cert_path}\n  key:  {key_path}");
    let tls = match tls::TlsConfig::load(&cert_path, &key_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "[tls] config error: {e}\n\nHint:\n- Set env vars GURT_CERT and GURT_KEY to your cert/key paths\n- Or generate dev certs with mkcert and run:\n    mkcert -install\n    mkcert localhost 127.0.0.1 ::1\n    export GURT_CERT=./localhost+2.pem\n    export GURT_KEY=./localhost+2-key.pem\n"
            );
            std::process::exit(1);
        }
    };
    let acceptor = tls.into_acceptor();

    let listener = TcpListener::bind(&addr).await?;
    eprintln!("gurtd listening on gurt://{}", addr);

    loop {
        let (stream, peer) = listener.accept().await?;
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_conn(stream, acceptor, peer).await {
                eprintln!(
                    "[tls] connection {peer} error: {err}\n  note: if client saw 'UnknownCA', ensure the client trusts the server certificate/CA"
                );
            }
        });
    }
}

async fn handle_conn(
    mut tcp: tokio::net::TcpStream,
    acceptor: tokio_rustls::TlsAcceptor,
    peer: SocketAddr,
) -> Result<()> {
    // Stage 1: plaintext HANDSHAKE (per docs)
    proto::handshake::read_and_respond_handshake(&mut tcp).await?;

    // Stage 2: upgrade to TLS 1.3 + ALPN GURT/1.0
    let mut tls_stream = match acceptor.accept(tcp).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[tls] accept error from {peer}: {e}");
            return Err(e.into());
        }
    };
    // Require TLS 1.3 per protocol requirements
    let (_, conn) = tls_stream.get_ref();
    // Log negotiated parameters
    let alpn = conn
        .alpn_protocol()
        .map(|p| String::from_utf8_lossy(p).to_string())
        .unwrap_or_else(|| "<none>".to_string());
    let sni = "<none>";
    let suite = conn
        .negotiated_cipher_suite()
        .map(|cs| format!("{:?}", cs))
        .unwrap_or_else(|| "<none>".to_string());
    eprintln!(
        "[tls] handshake ok from {peer}: version={:?} alpn={} sni={} cipher={}",
        conn.protocol_version(),
        alpn,
        sni,
        suite
    );
    if conn.protocol_version() != Some(ProtocolVersion::TLSv1_3) {
        // Drop connection if not TLS 1.3
        eprintln!(
            "[tls] dropping {peer}: negotiated version {:?} (require TLSv1.3)",
            conn.protocol_version()
        );
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

    let response = router::handle_with_peer(req, Some(peer))?;
    let bytes = response.into_bytes();
    tls_stream.write_all(&bytes).await?;
    Ok(())
}
