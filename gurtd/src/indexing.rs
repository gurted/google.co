use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::{timeout, Duration};
use tokio_rustls::client::TlsStream;

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use gurt_api::limits::{enforce_max_message_size, MAX_MESSAGE_BYTES};
use rustls::{DigitallySignedStruct, SignatureScheme};

use crate::crawler::client::ClientResponse;
use crate::crawler::pipeline::{process_fetched_document, DynamicReCrawlQueue};
use crate::crawler::sitemap::parse_sitemap_xml;
use crate::services;
use serde_json::json;
use tokio::time::timeout as tokio_timeout;
use std::collections::HashMap;
use std::time::{Duration as StdDuration, Instant};

const DEFAULT_PORT: u16 = 4878;
const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_HANDSHAKE_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_FETCH_TIMEOUT_MS: u64 = 30_000;
const DNS_TIMEOUT: Duration = Duration::from_secs(2);
const DNS_CACHE_TTL: StdDuration = StdDuration::from_secs(60);
const DEFAULT_READ_IDLE_MS: u64 = 500;
const MIN_READ_IDLE_MS: u64 = 100;
const MAX_READ_IDLE_MS: u64 = 5_000;
const RENDER_BUDGET: Duration = Duration::from_millis(120);
const MAX_PAGES_PER_DOMAIN: usize = 16;

/// Public entry point used by the router when a new domain submission arrives.
pub fn enqueue_domain(domain: String) {
    if domain.is_empty() {
        return;
    }
    INDEXING_SERVICE.enqueue(domain);
}

static INDEXING_SERVICE: Lazy<IndexingService> = Lazy::new(IndexingService::new);
static RECRAWL_QUEUE: Lazy<DynamicReCrawlQueue> = Lazy::new(DynamicReCrawlQueue::new);

struct IndexingService {
    sender: Mutex<Option<UnboundedSender<IndexJob>>>,
    in_flight: Arc<Mutex<HashSet<String>>>,
}

impl IndexingService {
    fn new() -> Self {
        Self {
            sender: Mutex::new(None),
            in_flight: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn enqueue(&self, domain: String) {
        // lock for the race conditions
        {
            let mut guard = self.in_flight.lock().unwrap();
            if guard.contains(&domain) {
                return;
            }
            guard.insert(domain.clone());
        }

        let sender = self.ensure_worker();
        if sender
            .send(IndexJob {
                domain: domain.clone(),
            })
            .is_err()
        {
            // unlock
            let mut guard = self.in_flight.lock().unwrap();
            guard.remove(&domain);
        }
    }

    fn ensure_worker(&self) -> UnboundedSender<IndexJob> {
        let mut guard = self.sender.lock().unwrap();
        if let Some(tx) = guard.as_ref() {
            return tx.clone();
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let in_flight = self.in_flight.clone();
        std::thread::Builder::new()
            .name("gurt-indexer".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("indexing runtime");
                runtime.block_on(run_worker(rx, in_flight));
            })
            .expect("spawn indexing worker");
        *guard = Some(tx.clone());
        tx
    }
}

struct IndexJob {
    domain: String,
}

async fn run_worker(mut rx: UnboundedReceiver<IndexJob>, in_flight: Arc<Mutex<HashSet<String>>>) {
    while let Some(job) = rx.recv().await {
        if let Err(err) = process_domain(&job.domain).await {
            eprintln!("[indexing] domain={} error={:?}", job.domain, err);
        }
        let mut guard = in_flight.lock().unwrap();
        guard.remove(&job.domain);
    }
}

async fn process_domain(domain: &str) -> Result<()> {
    eprintln!("[indexing] enqueue domain={}", domain);
    let urls = collect_candidate_urls(domain).await;
    if urls.is_empty() {
        return Err(anyhow!("no crawl candidates"));
    }

    for url in urls {
        if let Err(err) = index_single_url(&url).await {
            eprintln!("[indexing] url={} error={:?}", url, err);
        }
    }

    // make searchable asap
    let engine = services::index_engine();
    if let Err(err) = engine.commit() {
        eprintln!("[indexing] commit error: {err:?}");
    }
    if let Err(err) = engine.refresh() {
        eprintln!("[indexing] refresh error: {err:?}");
    }

    // drain any queued dynamic re-crawls just to log visibility for now
    let queued = RECRAWL_QUEUE.len().await;
    if queued > 0 {
        let drained = RECRAWL_QUEUE.drain().await;
        for item in drained {
            eprintln!(
                "[indexing] dynamic requeue url={} reason={:?}",
                item.url, item.reason
            );
        }
    }

    Ok(())
}

async fn collect_candidate_urls(domain: &str) -> Vec<String> {
    let mut urls = vec![format!("gurt://{domain}/")];
    let sitemap_url = format!("gurt://{domain}/sitemap.xml");
    if let Ok(resp) = fetch_gurt(&sitemap_url).await {
        if (200..300).contains(&resp.code) {
            if let Ok(xml) = String::from_utf8(resp.body.clone()) {
                let entries = parse_sitemap_xml(&xml);
                for entry in entries {
                    if urls.len() >= MAX_PAGES_PER_DOMAIN {
                        break;
                    }
                    if let Some(normalized) = normalize_candidate_url(domain, entry) {
                        urls.push(normalized);
                    }
                }
            }
        }
    }
    urls.sort();
    urls.dedup();
    urls.truncate(MAX_PAGES_PER_DOMAIN);
    urls
}

fn normalize_candidate_url(domain: &str, raw: String) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("gurt://") {
        return Some(trimmed.to_string());
    }
    if trimmed.starts_with('/') {
        return Some(format!("gurt://{}{}", domain, trimmed));
    }
    Some(format!("gurt://{}/{trimmed}", domain))
}

async fn index_single_url(url: &str) -> Result<()> {
    let resp = fetch_gurt(url).await?;
    if !(200..300).contains(&resp.code) {
        eprintln!("[indexing] fetch status={} url={} headers={:?}", resp.code, url, resp.headers);
        return Err(anyhow!("fetch status {}", resp.code));
    }
    let content_type = header_value(&resp.headers, "content-type");
    if let Some(ct) = content_type {
        if !ct.to_ascii_lowercase().contains("text/html") {
            return Ok(()); // skip non-HTML payloads rn
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
        &RECRAWL_QUEUE,
        url,
        domain,
        &title,
        &body,
        "en",
        fetch_time,
        RENDER_BUDGET,
    )
    .await?;
    Ok(())
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

async fn fetch_gurt(url: &str) -> Result<ClientResponse> {
    let parsed = url::Url::parse(url)?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("missing host"))?
        .to_string();
    let port = parsed.port().unwrap_or(DEFAULT_PORT);
    let path = format_request_path(&parsed);

    let connect_timeout = Duration::from_millis(
        std::env::var("GURT_CONNECT_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|v| v.clamp(500, 60_000))
            .unwrap_or(DEFAULT_CONNECT_TIMEOUT_MS),
    );
    let handshake_timeout = Duration::from_millis(
        std::env::var("GURT_HANDSHAKE_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|v| v.clamp(200, 30_000))
            .unwrap_or(DEFAULT_HANDSHAKE_TIMEOUT_MS),
    );
    let fetch_timeout = Duration::from_millis(
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
            // never attempt GURT DNS for localhost
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
            ConnectTarget::Ip(ip) => tokio_timeout(connect_timeout, tokio::net::TcpStream::connect((ip, port)))
                .await
                .map_err(|_| anyhow!("connect timeout"))?
                .with_context(|| format!("connect to {}:{}", ip, port))?,
            ConnectTarget::Host(h) => tokio_timeout(connect_timeout, tokio::net::TcpStream::connect((h.as_str(), port)))
                .await
                .map_err(|_| anyhow!("connect timeout"))?
                .with_context(|| format!("connect to {}:{}", h, port))?,
        };
        tcp.set_nodelay(true).ok();
        debug_log(|| format!("[indexing] handshake start host={}", host));
        tokio_timeout(handshake_timeout, perform_handshake(&mut tcp, &host))
            .await
            .map_err(|_| anyhow!("handshake timeout"))??;

        let connector = tls_connector();
        let server_name = server_name_from_host(&host)?;
        debug_log(|| "[indexing] tls connect".to_string());
        let mut tls = tokio_timeout(handshake_timeout, connector.connect(server_name, tcp))
            .await
            .map_err(|_| anyhow!("tls connect timeout"))??;

        debug_log(|| format!("[indexing] send request path={}", path));
        send_request(&mut tls, &host, port, &path).await?;
        let resp = read_response(&mut tls).await?;
        Ok(resp)
    };

    timeout(fetch_timeout, fut).await.unwrap_or_else(|_| Err(anyhow!("fetch timeout")))
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

async fn perform_handshake(stream: &mut tokio::net::TcpStream, host: &str) -> Result<()> {
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

async fn read_response(stream: &mut TlsStream<tokio::net::TcpStream>) -> Result<ClientResponse> {
    let read_idle_ms: u64 = std::env::var("GURT_READ_IDLE_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|v| v.clamp(MIN_READ_IDLE_MS, MAX_READ_IDLE_MS))
        .unwrap_or(DEFAULT_READ_IDLE_MS);
    let read_idle_timeout = Duration::from_millis(read_idle_ms);
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
    debug_log(|| format!("[indexing] recv headers content-length={:?}", content_length));

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
        if body.len() > len { body.truncate(len); }
        if body.len() < len {
            debug_log(|| format!(
                "[indexing] body truncated: expected={} got={}",
                len, body.len()
            ));
        } else {
            debug_log(|| format!("[indexing] body length={} (content-length)", body.len()));
        }
    } else {
        // mo content-length provided: read until EOF or idle timeout
        loop {
            match tokio_timeout(read_idle_timeout, stream.read(&mut tmp)).await {
                Ok(Ok(n)) => {
                    if n == 0 {
                        break;
                    }
                    body.extend_from_slice(&tmp[..n]);
                    enforce_max_message_size(header_end + 4 + body.len())?;
                }
                Ok(Err(_)) => break,
                Err(_) => {
                    // idle timeout
                    break;
                }
            }
        }
        debug_log(|| format!("[indexing] body length={} (read-until-idle)", body.len()));
    }

    if std::env::var("GURT_DEBUG_BODY").ok().filter(|v| v != "0").is_some() {
        let preview_len = body.len().min(2048);
        let preview = String::from_utf8_lossy(&body[..preview_len]);
        let sanitized = preview.replace('\n', "\\n").replace('\r', "");
        debug_log(|| format!("[indexing] body preview ({} bytes): {}{}",
            preview_len,
            &sanitized,
            if body.len() > preview_len { " ...<truncated>" } else { "" }
        ));
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

fn server_name_from_host(
    host: &str,
) -> Result<rustls::pki_types::ServerName<'static>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        Ok(rustls::pki_types::ServerName::IpAddress(ip.into()))
    } else {
        rustls::pki_types::ServerName::try_from(host.to_owned())
            .map_err(|_| anyhow!("invalid server name"))
    }
}

#[cfg(test)]
mod tests {
    use super::{pick_ip_from_dns_response, pick_cname_from_dns_response};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn picks_ipv4_a_record_first() {
        let body = br#"{
            "name": "api.blog.example.web",
            "tld": "web",
            "records": [
                {"id":1,"type":"A","name":"api.blog","value":"192.168.1.100","ttl":3600}
            ]
        }"#;
        let ip = pick_ip_from_dns_response(body).unwrap();
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::new(192,168,1,100)));
    }

    #[test]
    fn falls_back_to_ipv6_aaaa() {
        let body = br#"{
            "name": "x.web",
            "tld": "web",
            "records": [
                {"id":2,"type":"AAAA","name":"x","value":"2001:db8::1","ttl":3600}
            ]
        }"#;
        let ip = pick_ip_from_dns_response(body).unwrap();
        assert_eq!(ip, IpAddr::V6(Ipv6Addr::new(0x2001,0x0db8,0,0,0,0,0,1)));
    }

    #[test]
    fn returns_none_when_no_address_records() {
        let body = br#"{
            "name": "x.web",
            "tld": "web",
            "records": [
                {"id":3,"type":"TXT","name":"x","value":"hello","ttl":60}
            ]
        }"#;
        let ip = pick_ip_from_dns_response(body);
        assert!(ip.is_none());
    }

    #[test]
    fn extracts_cname_target() {
        let body = br#"{
            "name": "www.example.web",
            "tld": "web",
            "records": [
                {"id":10,"type":"CNAME","name":"www","value":"example.web.","ttl":300}
            ]
        }"#;
        let cname = pick_cname_from_dns_response(body);
        assert_eq!(cname.as_deref(), Some("example.web"));
    }
}

enum ConnectTarget {
    Ip(IpAddr),
    Host(String),
}

fn dns_service_endpoint() -> (String, Option<IpAddr>, u16) {
    let host = std::env::var("GURT_DNS_HOST").unwrap_or_else(|_| "dns.web".to_string());
    let addr = std::env::var("GURT_DNS_ADDR")
        .ok()
        .and_then(|s| s.parse::<IpAddr>().ok());
    let port = std::env::var("GURT_DNS_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);
    (host, addr, port)
}

async fn resolve_via_gurt_dns(domain: &str) -> Option<IpAddr> {
    if let Some(ip) = dns_cache_get(domain) {
        debug_log(|| format!("[indexing] dns cache hit domain={} ip={}", domain, ip));
        return Some(ip);
    }
    let (dns_host, dns_addr, dns_port) = dns_service_endpoint();
    debug_log(|| format!(
        "[indexing] dns resolve domain={} via host={} addr={:?} port={}",
        domain, dns_host, dns_addr, dns_port
    ));

    let mut current = domain.to_string();
    let original = current.clone();
    let mut depth = 0usize;
    const MAX_CNAME_DEPTH: usize = 5;
    while depth < MAX_CNAME_DEPTH {
        depth += 1;
        // build JSON body per docs (no record_type filter to allow AAAA-only domains)
        let body_val = json!({
            "domain": current,
        });
        let body = match serde_json::to_vec(&body_val) { Ok(b) => b, Err(_) => return None };

        // one resolution exchange with its own timeout
        let work = async {
        // Connect to DNS service
        let mut tcp = match match dns_addr {
            Some(ip) => tokio::net::TcpStream::connect((ip, dns_port)).await,
            None => tokio::net::TcpStream::connect((dns_host.as_str(), dns_port)).await,
        } {
            Ok(s) => s,
            Err(_) => return None,
        };
        tcp.set_nodelay(true).ok();
        if perform_handshake(&mut tcp, &dns_host).await.is_err() {
            return None;
        }
        let connector = tls_connector();
        let server_name = match server_name_from_host(&dns_host) {
            Ok(n) => n,
            Err(_) => return None,
        };
        let mut tls = match connector.connect(server_name, tcp).await {
            Ok(t) => t,
            Err(_) => return None,
        };

        if send_request_with_body(
            &mut tls,
            &dns_host,
            "/resolve-full",
            "POST",
            &[
                ("content-type", "application/json"),
                ("accept", "application/json"),
            ],
            &body,
        )
        .await
        .is_err()
        {
            return None;
        }
        let resp = match read_response(&mut tls).await {
            Ok(r) => r,
            Err(_) => return None,
        };
        if resp.code < 200 || resp.code >= 300 {
            return None;
        }
            // prefer immediate A/AAAA answers
            if let Some(ip) = pick_ip_from_dns_response(&resp.body) {
                dns_cache_put(&current, ip);
                return Some(ip);
            }
            // otherwise, see if there's a CNAME to follow; outer loop will continue
            if let Some(next) = pick_cname_from_dns_response(&resp.body) {
                debug_log(|| format!("[indexing] dns cname {} -> {}", current, next));
                // indicate to outer scope to update `current`
                return Some(match next.parse::<IpAddr>() {
                    Ok(ip) => ip, // unlikely: CNAME to literal IP, but support it
                    Err(_) => {
                        // use a sentinel by writing into cache for the alias to avoid re-querying if it repeats
                        // and return None to signal outer to set `current = next`.
                        // we cannot pass the string here, so return a special value via None outside.
                        return None;
                    }
                });
            }
            None
        };
        match tokio_timeout(DNS_TIMEOUT, work).await {
            Ok(Some(ip)) => {
                // either we obtained final IP or CNAME resolved to IP; cache for original too
                dns_cache_put(&current, ip);
                dns_cache_put(&original, ip);
                return Some(ip);
            }
            Ok(None) => {
                // mo IP returned; try to parse CNAME by issuing another request is unnecessary now,
                // because the same request already checked for CNAME. Proceed to next iteration by
                // updating `current` if possible via a quick parse request.
                // re-run minimally to get the cname string here.
                let body_val = json!({ "domain": current });
                let body = match serde_json::to_vec(&body_val) { Ok(b) => b, Err(_) => return None };
                let next = tokio_timeout(DNS_TIMEOUT, async {
                    let mut tcp = match match dns_addr {
                        Some(ip) => tokio::net::TcpStream::connect((ip, dns_port)).await,
                        None => tokio::net::TcpStream::connect((dns_host.as_str(), dns_port)).await,
                    } { Ok(s) => s, Err(_) => return None };
                    tcp.set_nodelay(true).ok();
                    if perform_handshake(&mut tcp, &dns_host).await.is_err() { return None; }
                    let connector = tls_connector();
                    let server_name = server_name_from_host(&dns_host).ok()?;
                    let mut tls = connector.connect(server_name, tcp).await.ok()?;
                    if send_request_with_body(&mut tls, &dns_host, "/resolve-full", "POST",
                        &[("content-type","application/json"),("accept","application/json")], &body).await.is_err() { return None; }
                    let resp = read_response(&mut tls).await.ok()?;
                    pick_cname_from_dns_response(&resp.body)
                }).await.ok().flatten();
                if let Some(next) = next { current = next; continue; }
                break;
            }
            Err(_) => {
                debug_log(|| format!("[indexing] dns resolve timeout domain={}", current));
                return None;
            }
        }
    }
    None
}

fn pick_ip_from_dns_response(body: &[u8]) -> Option<IpAddr> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let records = v.get("records")?.as_array()?;
    // prefer IPv4 A records first, then IPv6 AAAA
    for rec in records {
        let typ = rec.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if typ.eq_ignore_ascii_case("A") {
            if let Some(val) = rec.get("value").and_then(|x| x.as_str()) {
                if let Ok(ip) = val.parse::<IpAddr>() {
                    if matches!(ip, IpAddr::V4(_)) {
                        return Some(ip);
                    }
                }
            }
        }
    }
    for rec in records {
        let typ = rec.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if typ.eq_ignore_ascii_case("AAAA") {
            if let Some(val) = rec.get("value").and_then(|x| x.as_str()) {
                if let Ok(ip) = val.parse::<IpAddr>() {
                    return Some(ip);
                }
            }
        }
    }
    None
}

fn pick_cname_from_dns_response(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let records = v.get("records")?.as_array()?;
    for rec in records {
        let typ = rec.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if typ.eq_ignore_ascii_case("CNAME") {
            if let Some(val) = rec.get("value").and_then(|x| x.as_str()) {
                let target = val.trim().trim_end_matches('.').to_string();
                if !target.is_empty() {
                    return Some(target);
                }
            }
        }
    }
    None
}

async fn send_request_with_body(
    stream: &mut TlsStream<tokio::net::TcpStream>,
    host: &str,
    path: &str,
    method: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) -> Result<()> {
    let mut req = format!(
        "{} {} GURT/1.0.0\r\nhost: {}\r\n",
        method, path, host
    );
    for (k, v) in extra_headers {
        req.push_str(k);
        req.push_str(": ");
        req.push_str(v);
        req.push_str("\r\n");
    }
    if !extra_headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-length")) {
        req.push_str(&format!("content-length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    let mut bytes = req.into_bytes();
    bytes.extend_from_slice(body);
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}

fn debug_log<F>(f: F)
where
    F: FnOnce() -> String,
{
    static ENABLED: once_cell::sync::Lazy<bool> = once_cell::sync::Lazy::new(|| {
        std::env::var("GURT_DEBUG_INDEX").ok().filter(|v| v != "0").is_some()
    });
    if *ENABLED {
        eprintln!("{}", f());
    }
}

static DNS_CACHE: once_cell::sync::Lazy<std::sync::Mutex<HashMap<String, (IpAddr, Instant)>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

fn dns_cache_get(domain: &str) -> Option<IpAddr> {
    let mut map = DNS_CACHE.lock().ok()?;
    if let Some((ip, t)) = map.get(domain) {
        if t.elapsed() <= DNS_CACHE_TTL {
            return Some(*ip);
        }
    }
    // expired
    map.remove(domain);
    None
}

fn dns_cache_put(domain: &str, ip: IpAddr) {
    if let Ok(mut map) = DNS_CACHE.lock() {
        map.insert(domain.to_string(), (ip, Instant::now()));
    }
}

fn tls_connector() -> tokio_rustls::TlsConnector {
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
