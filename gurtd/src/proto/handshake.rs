use anyhow::{anyhow, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAX_HANDSHAKE_BYTES: usize = 8 * 1024; // 8KB cap for safety
const PROTO_VERSION: &str = "GURT/1.0.0";

pub async fn read_and_respond_handshake<S>(stream: &mut S) -> Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let mut buf = Vec::with_capacity(512);
    let mut tmp = [0u8; 512];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 { return Err(anyhow!("handshake: connection closed")); }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > MAX_HANDSHAKE_BYTES {
            return Err(anyhow!("handshake: too large"));
        }
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        // continue reading
    }

    // Validate first line: HANDSHAKE / GURT/1.0.0
    let text = String::from_utf8_lossy(&buf);
    let first_line = text.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    let version = parts.next().unwrap_or("");
    if method != "HANDSHAKE" || path != "/" || version != PROTO_VERSION {
        return Err(anyhow!("invalid handshake start-line: {first_line}"));
    }

    // Respond 101 SWITCHING_PROTOCOLS with required headers
    let date = httpdate::fmt_http_date(std::time::SystemTime::now());
    let resp = format!(
        "{ver} 101 SWITCHING_PROTOCOLS\r\n\
gurt-version: 1.0.0\r\n\
encryption: TLS/1.3\r\n\
alpn: GURT/1.0\r\n\
server: GURT/1.0.0\r\n\
date: {date}\r\n\r\n",
        ver = PROTO_VERSION,
        date = date,
    );
    stream.write_all(resp.as_bytes()).await?;
    Ok(())
}

