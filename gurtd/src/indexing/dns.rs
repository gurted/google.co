use anyhow::anyhow;
use once_cell::sync::Lazy;
use serde_json::json;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration as StdDuration, Instant};
use tokio::io::AsyncWriteExt;

use super::fetch::tls_connector;

const DNS_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(2);
const DNS_CACHE_TTL: StdDuration = StdDuration::from_secs(60);

pub fn dns_service_endpoint() -> (String, Option<IpAddr>, u16) {
    let host = std::env::var("GURT_DNS_HOST").unwrap_or_else(|_| "dns.web".to_string());
    let addr = std::env::var("GURT_DNS_ADDR").ok().and_then(|s| s.parse::<IpAddr>().ok());
    let port = std::env::var("GURT_DNS_PORT").ok().and_then(|s| s.parse::<u16>().ok()).unwrap_or(super::DEFAULT_PORT);
    (host, addr, port)
}

pub async fn resolve_via_gurt_dns(domain: &str) -> Option<IpAddr> {
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
        let body_val = json!({ "domain": current });
        let body = match serde_json::to_vec(&body_val) { Ok(b) => b, Err(_) => return None };

        let work = async {
            let mut tcp = match match dns_addr {
                Some(ip) => tokio::net::TcpStream::connect((ip, dns_port)).await,
                None => tokio::net::TcpStream::connect((dns_host.as_str(), dns_port)).await,
            } { Ok(s) => s, Err(_) => return None };
            tcp.set_nodelay(true).ok();
            if super::fetch::perform_handshake(&mut tcp, &dns_host).await.is_err() { return None; }
            let connector = tls_connector();
            let server_name = match server_name_from_host(&dns_host) { Ok(n) => n, Err(_) => return None };
            let mut tls = match connector.connect(server_name, tcp).await { Ok(t) => t, Err(_) => return None };
            if send_request_with_body(
                &mut tls,
                &dns_host,
                "/resolve-full",
                "POST",
                &[("content-type", "application/json"), ("accept", "application/json")],
                &body,
            )
            .await
            .is_err() { return None; }
            let resp = match super::fetch::read_response(&mut tls).await { Ok(r) => r, Err(_) => return None };
            if resp.code < 200 || resp.code >= 300 { return None; }
            if let Some(ip) = pick_ip_from_dns_response(&resp.body) { dns_cache_put(&current, ip); return Some(ip); }
            if let Some(next) = pick_cname_from_dns_response(&resp.body) {
                debug_log(|| format!("[indexing] dns cname {} -> {}", current, next));
                // Update outer current
                return None; // signal to outer loop to update current via second request below
            }
            None
        };
        match tokio::time::timeout(DNS_TIMEOUT, work).await {
            Ok(Some(ip)) => {
                dns_cache_put(&current, ip);
                dns_cache_put(&original, ip);
                return Some(ip);
            }
            Ok(None) => {
                // obtain CNAME explicitly
                let body_val = json!({ "domain": current });
                let body = match serde_json::to_vec(&body_val) { Ok(b) => b, Err(_) => return None };
                let next = tokio::time::timeout(DNS_TIMEOUT, async {
                    let mut tcp = match match dns_addr {
                        Some(ip) => tokio::net::TcpStream::connect((ip, dns_port)).await,
                        None => tokio::net::TcpStream::connect((dns_host.as_str(), dns_port)).await,
                    } { Ok(s) => s, Err(_) => return None };
                    tcp.set_nodelay(true).ok();
                    if super::fetch::perform_handshake(&mut tcp, &dns_host).await.is_err() { return None; }
                    let connector = tls_connector();
                    let server_name = server_name_from_host(&dns_host).ok()?;
                    let mut tls = connector.connect(server_name, tcp).await.ok()?;
                    if send_request_with_body(&mut tls, &dns_host, "/resolve-full", "POST",
                        &[("content-type","application/json"),("accept","application/json")], &body).await.is_err() { return None; }
                    let resp = super::fetch::read_response(&mut tls).await.ok()?;
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

pub fn pick_ip_from_dns_response(body: &[u8]) -> Option<IpAddr> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let records = v.get("records")?.as_array()?;
    for rec in records {
        let typ = rec.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if typ.eq_ignore_ascii_case("A") {
            if let Some(val) = rec.get("value").and_then(|x| x.as_str()) {
                if let Ok(ip) = val.parse::<IpAddr>() { if matches!(ip, IpAddr::V4(_)) { return Some(ip); } }
            }
        }
    }
    for rec in records {
        let typ = rec.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if typ.eq_ignore_ascii_case("AAAA") {
            if let Some(val) = rec.get("value").and_then(|x| x.as_str()) {
                if let Ok(ip) = val.parse::<IpAddr>() { return Some(ip); }
            }
        }
    }
    None
}

pub fn pick_cname_from_dns_response(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let records = v.get("records")?.as_array()?;
    for rec in records {
        let typ = rec.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if typ.eq_ignore_ascii_case("CNAME") {
            if let Some(val) = rec.get("value").and_then(|x| x.as_str()) {
                let target = val.trim().trim_end_matches('.').to_string();
                if !target.is_empty() { return Some(target); }
            }
        }
    }
    None
}

pub fn server_name_from_host(host: &str) -> anyhow::Result<rustls::pki_types::ServerName<'static>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        Ok(rustls::pki_types::ServerName::IpAddress(ip.into()))
    } else {
        rustls::pki_types::ServerName::try_from(host.to_owned()).map_err(|_| anyhow!("invalid server name"))
    }
}

async fn send_request_with_body(
    stream: &mut tokio_rustls::client::TlsStream<tokio::net::TcpStream>,
    host: &str,
    path: &str,
    method: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) -> anyhow::Result<()> {
    let mut req = format!("{} {} GURT/1.0.0\r\nhost: {}\r\n", method, path, host);
    for (k, v) in extra_headers { req.push_str(k); req.push_str(": "); req.push_str(v); req.push_str("\r\n"); }
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

fn debug_log<F>(f: F) where F: FnOnce() -> String {
    static ENABLED: once_cell::sync::Lazy<bool> = once_cell::sync::Lazy::new(|| {
        std::env::var("GURT_DEBUG_INDEX").ok().filter(|v| v != "0").is_some()
    });
    if *ENABLED { eprintln!("{}", f()); }
}

static DNS_CACHE: Lazy<std::sync::Mutex<HashMap<String, (IpAddr, Instant)>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

fn dns_cache_get(domain: &str) -> Option<IpAddr> {
    let mut map = DNS_CACHE.lock().ok()?;
    if let Some((ip, t)) = map.get(domain) { if t.elapsed() <= DNS_CACHE_TTL { return Some(*ip); } }
    map.remove(domain);
    None
}

fn dns_cache_put(domain: &str, ip: IpAddr) {
    if let Ok(mut map) = DNS_CACHE.lock() { map.insert(domain.to_string(), (ip, Instant::now())); }
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
