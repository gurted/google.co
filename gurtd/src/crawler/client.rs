use anyhow::Result;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;

use gurt_api::limits::{enforce_max_message_size, MAX_MESSAGE_BYTES};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    InvalidMessage,
    Connection,
    Timeout,
    Io,
}

pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> IoStream for T {}
pub type DynStream = Pin<Box<dyn IoStream>>;

pub type ConnectorFn = dyn Fn(
        &str,
        u16,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<DynStream, ClientError>> + Send>>
    + Send
    + Sync;

#[derive(Clone)]
pub struct GurtClient {
    connector: Arc<ConnectorFn>,
    pub req_timeout: Duration,
    pub retry_backoff: Duration,
    pub header_read_chunk: usize,
}

impl GurtClient {
    pub fn new_with_connector(connector: Arc<ConnectorFn>) -> Self {
        Self {
            connector,
            req_timeout: Duration::from_secs(2),
            retry_backoff: Duration::from_millis(10),
            header_read_chunk: 2048,
        }
    }

    /// Create a client that does not actually perform network I/O (for tests),
    /// expecting the provided connector to handle streams (e.g., via tokio::io::duplex).
    pub fn new_test(connector: Arc<ConnectorFn>) -> Self {
        Self::new_with_connector(connector)
    }

    /// Build a client with a rustls-based TLS connector. For development only (no cert verification).
    #[cfg(feature = "tls_client")]
    pub fn new_rustls_insecure() -> Self {
        use rustls::pki_types::ServerName;
        use rustls::ClientConfig;
        use std::sync::Arc as StdArc;
        use tokio_rustls::TlsConnector;

        let mut cfg = ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(StdArc::new(NoVerifier))
            .with_no_client_auth();
        cfg.alpn_protocols = vec![b"GURT/1.0".to_vec()];
        let cfg = StdArc::new(cfg);
        let tls = TlsConnector::from(cfg);
        let connector_arc: Arc<ConnectorFn> = Arc::new(move |host: &str, port: u16| {
            let host_owned = host.to_string();
            let tls = tls.clone();
            Box::pin(async move {
                let tcp = tokio::net::TcpStream::connect((host_owned.as_str(), port))
                    .await
                    .map_err(|_| ClientError::Connection)?;
                let server_name = ServerName::try_from(host_owned.as_str())
                    .map_err(|_| ClientError::InvalidMessage)?;
                let stream = tls
                    .connect(server_name, tcp)
                    .await
                    .map_err(|_| ClientError::Connection)?;
                Ok(Box::pin(stream) as DynStream)
            })
        });
        Self::new_with_connector(connector_arc)
    }

    pub async fn fetch_with_retries(
        &self,
        url: &str,
        retries: usize,
    ) -> Result<ClientResponse, ClientError> {
        let mut last_err = None;
        for attempt in 0..=retries {
            match self.fetch_once(url).await {
                Ok(resp) => return Ok(resp),
                Err(e @ ClientError::InvalidMessage) => return Err(e),
                Err(e @ ClientError::Connection)
                | Err(e @ ClientError::Timeout)
                | Err(e @ ClientError::Io) => {
                    last_err = Some(e);
                    // simple backoff (configurable)
                    if attempt < retries {
                        tokio::time::sleep(self.retry_backoff).await;
                    }
                }
            }
        }
        Err(last_err.unwrap_or(ClientError::Connection))
    }

    async fn fetch_once(&self, url: &str) -> Result<ClientResponse, ClientError> {
        // Parse gurt:// URL
        let parsed = url::Url::parse(url).map_err(|_| ClientError::InvalidMessage)?;
        if parsed.scheme() != "gurt" {
            return Err(ClientError::InvalidMessage);
        }
        let host = parsed.host_str().ok_or(ClientError::InvalidMessage)?;
        let port = parsed.port().unwrap_or(4878);
        let path = format!(
            "{}{}",
            parsed.path(),
            parsed
                .query()
                .map(|q| format!("?{}", q))
                .unwrap_or_default()
        );

        // Connect
        let fut = (self.connector)(host, port);
        let mut stream = timeout(self.req_timeout, fut)
            .await
            .map_err(|_| ClientError::Timeout)??;

        // Handshake: send a simple token, expect GURT 101 response headers
        let hs_req = b"HANDSHAKE / GURT/1.0\r\n\r\n";
        let _ = timeout(self.req_timeout, stream.write_all(hs_req))
            .await
            .map_err(|_| ClientError::Timeout)
            .map_err(|_| ClientError::Timeout)?;
        let _ = timeout(self.req_timeout, stream.flush())
            .await
            .map_err(|_| ClientError::Timeout);
        let hs = read_response_like(&mut stream, self.header_read_chunk).await?;
        if hs.code != 101 {
            return Err(ClientError::InvalidMessage);
        }

        // Request
        let req = format!("GET {} GURT/1.0\r\nhost: {}\r\n\r\n", path, host);
        let _ = timeout(self.req_timeout, stream.write_all(req.as_bytes()))
            .await
            .map_err(|_| ClientError::Timeout)?;
        let _ = timeout(self.req_timeout, stream.flush())
            .await
            .map_err(|_| ClientError::Timeout);

        // Response
        let resp = read_response_like(&mut stream, self.header_read_chunk).await?;
        Ok(resp)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientResponse {
    pub code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

async fn read_response_like(
    stream: &mut DynStream,
    chunk: usize,
) -> Result<ClientResponse, ClientError> {
    // Read header up to CRLFCRLF; enforce total cap
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut tmp = vec![0u8; chunk.max(1)];
    let mut header_end: Option<usize> = None;
    loop {
        let n = stream.read(&mut tmp).await.map_err(|_| ClientError::Io)?;
        if n == 0 {
            return Err(ClientError::Connection);
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > MAX_MESSAGE_BYTES {
            return Err(ClientError::Io);
        }
        if let Some(pos) = find_crlfcrlf(&buf) {
            header_end = Some(pos);
            break;
        }
    }
    let header_end = header_end.ok_or(ClientError::InvalidMessage)?;
    let (head, rest) = buf.split_at(header_end + 4);
    let head_str = std::str::from_utf8(head).map_err(|_| ClientError::InvalidMessage)?;
    let mut lines = head_str.split("\r\n");
    let status = lines.next().unwrap_or("");
    let mut sp = status.split_whitespace();
    let _proto = sp.next().unwrap_or("");
    let code = sp
        .next()
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or(ClientError::InvalidMessage)?;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut content_length: usize = 0;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                if let Ok(n) = value.parse::<usize>() {
                    content_length = n;
                }
            }
            headers.push((name, value));
        }
    }
    // Read body to content-length if present
    let mut body = Vec::new();
    if content_length > 0 {
        enforce_max_message_size(header_end + 4 + content_length).map_err(|_| ClientError::Io)?;
        if !rest.is_empty() {
            body.extend_from_slice(rest);
        }
        while body.len() < content_length {
            let mut chunk = [0u8; 4096];
            let n = stream.read(&mut chunk).await.map_err(|_| ClientError::Io)?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
            let total = header_end + 4 + body.len();
            if total > MAX_MESSAGE_BYTES {
                return Err(ClientError::Io);
            }
        }
        body.truncate(content_length);
    }
    Ok(ClientResponse {
        code,
        headers,
        body,
    })
}

fn find_crlfcrlf(buf: &[u8]) -> Option<usize> {
    // naive search
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

// Development-only certificate verifier (accepts any cert). Do not use in production.
#[cfg(feature = "tls_client")]
#[derive(Debug)]
struct NoVerifier;
#[cfg(feature = "tls_client")]
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
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        use rustls::SignatureScheme::*;
        vec![
            ECDSA_NISTP256_SHA256,
            ECDSA_NISTP384_SHA384,
            ED25519,
            RSA_PKCS1_SHA256,
            RSA_PKCS1_SHA384,
            RSA_PKCS1_SHA512,
            RSA_PSS_SHA256,
            RSA_PSS_SHA384,
            RSA_PSS_SHA512,
        ]
    }
}
