use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::client::TlsStream;

use std::net::IpAddr;

use gurt_api::limits::{enforce_max_message_size, MAX_MESSAGE_BYTES};
use rustls::{DigitallySignedStruct, SignatureScheme};

use crate::crawler::client::ClientResponse;
use crate::crawler::pipeline::{process_fetched_document, DynamicReCrawlQueue};
use crate::services;

use super::dns::{resolve_via_gurt_dns, server_name_from_host};

const DEFAULT_PORT: u16 = super::DEFAULT_PORT;
const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_HANDSHAKE_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_FETCH_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_READ_IDLE_MS: u64 = 500;
const MIN_READ_IDLE_MS: u64 = 100;
const MAX_READ_IDLE_MS: u64 = 5_000;

pub async fn index_single_url(url: &str, recrawl: &DynamicReCrawlQueue) -> Result<()> {
    let resp = fetch_gurt(url).await?;
    if !(200..300).contains(&resp.code) {
        eprintln!(
            "[indexing] fetch status={} url={} headers={:?}",
            resp.code, url, resp.headers
        );
        return Err(anyhow!("fetch status {}", resp.code));
    }
    let content_type = header_value(&resp.headers, "content-type");
    if let Some(ct) = content_type {
        if !ct.to_ascii_lowercase().contains("text/html") {
            return Ok(());
        }
    }
    let body = String::from_utf8(resp.body.clone())
        .unwrap_or_else(|_| String::from_utf8_lossy(&resp.body).to_string());
    let parsed = url::Url::parse(url)?;
    let domain = parsed.host_str().unwrap_or("");
    if domain.is_empty() {
        return Err(anyhow!("missing host"));
    }
    let title = extract_title(&body).unwrap_or_else(|| domain.to_string());
    let fetch_time = current_unix_timestamp();
    let engine = services::index_engine();
    process_fetched_document(
        engine,
        recrawl,
        url,
        domain,
        &title,
        &body,
        "en",
        fetch_time,
        super::RENDER_BUDGET,
    )
    .await?;
    Ok(())
}

pub async fn fetch_gurt(url: &str) -> Result<ClientResponse> {
    let parsed = url::Url::parse(url)?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("missing host"))?
        .to_string();
    let port = parsed.port().unwrap_or(DEFAULT_PORT);
    let path = format_request_path(&parsed);

    let connect_timeout = tokio::time::Duration::from_millis(
        std::env::var("GURT_CONNECT_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|v| v.clamp(500, 60_000))
            .unwrap_or(DEFAULT_CONNECT_TIMEOUT_MS),
    );
    let handshake_timeout = tokio::time::Duration::from_millis(
        std::env::var("GURT_HANDSHAKE_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|v| v.clamp(200, 30_000))
            .unwrap_or(DEFAULT_HANDSHAKE_TIMEOUT_MS),
    );
    let fetch_timeout = tokio::time::Duration::from_millis(
        std::env::var("GURT_FETCH_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|v| v.clamp(1_000, 120_000))
            .unwrap_or(DEFAULT_FETCH_TIMEOUT_MS),
    );

    let fut = async move {
        // direct IP > GURT DNS > OS DNS (fallback)
        debug_log(|| format!("[indexing] resolve host={}", host));
        let connect_target: ConnectTarget = if let Ok(ip) = host.parse::<IpAddr>() {
            ConnectTarget::Ip(ip)
        } else if host.eq_ignore_ascii_case("localhost") {
            ConnectTarget::Host(host.clone())
        } else if let Some(ip) = resolve_via_gurt_dns(&host).await {
            ConnectTarget::Ip(ip)
        } else {
            ConnectTarget::Host(host.clone())
        };

        debug_log(|| match &connect_target {
            ConnectTarget::Ip(ip) => format!("[indexing] connect target ip={} port={}", ip, port),
            ConnectTarget::Host(h) => format!("[indexing] connect target host={} port={}", h, port),
        });
        let mut tcp = match connect_target {
            ConnectTarget::Ip(ip) => {
                tokio::time::timeout(connect_timeout, tokio::net::TcpStream::connect((ip, port)))
                    .await
                    .map_err(|_| anyhow!("connect timeout"))?
                    .with_context(|| format!("connect to {}:{}", ip, port))?
            }
            ConnectTarget::Host(h) => tokio::time::timeout(
                connect_timeout,
                tokio::net::TcpStream::connect((h.as_str(), port)),
            )
            .await
            .map_err(|_| anyhow!("connect timeout"))?
            .with_context(|| format!("connect to {}:{}", h, port))?,
        };
        tcp.set_nodelay(true).ok();
        debug_log(|| format!("[indexing] handshake start host={}", host));
        tokio::time::timeout(handshake_timeout, perform_handshake(&mut tcp, &host))
            .await
            .map_err(|_| anyhow!("handshake timeout"))??;

        let connector = tls_connector();
        let server_name = server_name_from_host(&host)?;
        debug_log(|| "[indexing] tls connect".to_string());
        let mut tls = tokio::time::timeout(handshake_timeout, connector.connect(server_name, tcp))
            .await
            .map_err(|_| anyhow!("tls connect timeout"))??;

        debug_log(|| format!("[indexing] send request path={}", path));
        send_request(&mut tls, &host, port, &path).await?;
        let resp = read_response(&mut tls).await?;
        Ok(resp)
    };

    tokio::time::timeout(fetch_timeout, fut)
        .await
        .unwrap_or_else(|_| Err(anyhow!("fetch timeout")))
}

fn format_request_path(url: &url::Url) -> String {
    let mut path = url.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    path
}

pub(super) async fn perform_handshake(
    stream: &mut tokio::net::TcpStream,
    host: &str,
) -> Result<()> {
    let ua = std::env::var("GURT_USER_AGENT").unwrap_or_else(|_| "gurtd/0.1".to_string());
    let request = format!(
        "HANDSHAKE / GURT/1.0.0\r\nhost: {}\r\nuser-agent: {}\r\n\r\n",
        host, ua
    );
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;

    let mut buf = Vec::with_capacity(512);
    let mut tmp = [0u8; 256];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err(anyhow!("handshake closed"));
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > MAX_MESSAGE_BYTES {
            return Err(anyhow!("handshake too large"));
        }
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }

    let text = String::from_utf8_lossy(&buf);
    let first_line = text.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let version = parts.next().unwrap_or("");
    let code = parts.next().unwrap_or("");
    if version != "GURT/1.0.0" || code != "101" {
        return Err(anyhow!("unexpected handshake response: {}", first_line));
    }
    Ok(())
}

async fn send_request(
    stream: &mut TlsStream<tokio::net::TcpStream>,
    host: &str,
    port: u16,
    path: &str,
) -> Result<()> {
    let host_header = if port != DEFAULT_PORT {
        format!("{}:{}", host, port)
    } else {
        host.to_string()
    };
    let req = format!(
        "GET {} GURT/1.0.0\r\nhost: {}\r\nuser-agent: gurtd/0.1\r\naccept: text/html, */*\r\nconnection: close\r\n\r\n",
        path, host_header
    );
    stream.write_all(req.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

pub(super) async fn read_response(
    stream: &mut TlsStream<tokio::net::TcpStream>,
) -> Result<ClientResponse> {
    let read_idle_ms: u64 = std::env::var("GURT_READ_IDLE_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|v| v.clamp(MIN_READ_IDLE_MS, MAX_READ_IDLE_MS))
        .unwrap_or(DEFAULT_READ_IDLE_MS);
    let read_idle_timeout = tokio::time::Duration::from_millis(read_idle_ms);
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 2048];
    let mut header_end: Option<usize> = None;
    while header_end.is_none() {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err(anyhow!("response closed"));
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > MAX_MESSAGE_BYTES {
            return Err(anyhow!("response too large"));
        }
        if let Some(pos) = find_crlfcrlf(&buf) {
            header_end = Some(pos);
        }
    }
    let header_end = header_end.unwrap();
    let (head, rest) = buf.split_at(header_end + 4);
    let head_str = std::str::from_utf8(head)?;
    let mut lines = head_str.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut sp = status_line.split_whitespace();
    let _version = sp.next().unwrap_or("");
    let code = sp
        .next()
        .ok_or_else(|| anyhow!("missing status code"))?
        .parse::<u16>()?;
    debug_log(|| format!("[indexing] recv status={}", code));

    let mut headers: Vec<(String, String)> = Vec::new();
    let mut content_length: Option<usize> = None;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            let lname = name.trim().to_ascii_lowercase();
            let val = value.trim().to_string();
            if lname == "content-length" {
                if let Ok(n) = val.parse::<usize>() {
                    content_length = Some(n);
                }
            }
            headers.push((lname, val));
        }
    }
    debug_log(|| {
        format!(
            "[indexing] recv headers content-length={:?}",
            content_length
        )
    });

    let mut body = rest.to_vec();
    if let Some(len) = content_length {
        enforce_max_message_size(header_end + 4 + len)?;
        while body.len() < len {
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&tmp[..n]);
            enforce_max_message_size(header_end + 4 + body.len())?;
        }
        if body.len() > len {
            body.truncate(len);
        }
        if body.len() < len {
            debug_log(|| {
                format!(
                    "[indexing] body truncated: expected={} got={}",
                    len,
                    body.len()
                )
            });
        } else {
            debug_log(|| format!("[indexing] body length={} (content-length)", body.len()));
        }
    } else {
        loop {
            match tokio::time::timeout(read_idle_timeout, stream.read(&mut tmp)).await {
                Ok(Ok(n)) => {
                    if n == 0 {
                        break;
                    }
                    body.extend_from_slice(&tmp[..n]);
                    enforce_max_message_size(header_end + 4 + body.len())?;
                }
                Ok(Err(_)) => break,
                Err(_) => {
                    break;
                }
            }
        }
        debug_log(|| format!("[indexing] body length={} (read-until-idle)", body.len()));
    }

    if std::env::var("GURT_DEBUG_BODY")
        .ok()
        .filter(|v| v != "0")
        .is_some()
    {
        let preview_len = body.len().min(2048);
        let preview = String::from_utf8_lossy(&body[..preview_len]);
        let sanitized = preview.replace('\n', "\\n").replace('\r', "");
        debug_log(|| {
            format!(
                "[indexing] body preview ({} bytes): {}{}",
                preview_len,
                &sanitized,
                if body.len() > preview_len {
                    " ...<truncated>"
                } else {
                    ""
                }
            )
        });
    }

    Ok(ClientResponse {
        code,
        headers,
        body,
    })
}

fn find_crlfcrlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case(name) {
            Some(v.as_str())
        } else {
            None
        }
    })
}

fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let after_tag = &lower[start..];
    let gt = after_tag.find('>')?;
    let content_start = start + gt + 1;
    let after_start = &lower[content_start..];
    let end_rel = after_start.find("</title>")?;
    let content_end = content_start + end_rel;
    let slice = html.get(content_start..content_end)?;
    let collapsed = slice.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn current_unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(super) fn tls_connector() -> tokio_rustls::TlsConnector {
    static CONNECTOR: Lazy<tokio_rustls::TlsConnector> = Lazy::new(|| {
        use rustls::ClientConfig;
        use std::sync::Arc;
        let mut cfg = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
            .with_no_client_auth();
        cfg.alpn_protocols = vec![b"GURT/1.0".to_vec()];
        tokio_rustls::TlsConnector::from(Arc::new(cfg))
    });
    CONNECTOR.clone()
}

#[derive(Debug)]
struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PKCS1_SHA256,
        ]
    }
}

enum ConnectTarget {
    Ip(IpAddr),
    Host(String),
}

fn debug_log<F>(f: F)
where
    F: FnOnce() -> String,
{
    static ENABLED: once_cell::sync::Lazy<bool> = once_cell::sync::Lazy::new(|| {
        std::env::var("GURT_DEBUG_INDEX")
            .ok()
            .filter(|v| v != "0")
            .is_some()
    });
    if *ENABLED {
        eprintln!("{}", f());
    }
}
