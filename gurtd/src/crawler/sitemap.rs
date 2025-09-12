/// Extract URLs inside <loc>...</loc> tags. Whitespace is trimmed.
pub fn parse_sitemap_xml(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<loc") {
        let after_start = &rest[start..];
        // find closing '>' of <loc ...>
        if let Some(gt) = after_start.find('>') {
            let after_tag = &after_start[gt + 1..];
            if let Some(end) = after_tag.find("</loc>") {
                let url = &after_tag[..end];
                let u = url.trim().to_string();
                if !u.is_empty() { out.push(u); }
                rest = &after_tag[end + 6..]; // move past </loc>
                continue;
            }
        }
        // no proper <loc>...</loc> found; stop to avoid infinite loop
        break;
    }
    out
}

/// Fetch sitemap.xml from gurt://<domain>/sitemap.xml and parse URLs.
pub async fn fetch_sitemap_urls(client: &crate::crawler::client::GurtClient, domain: &str) -> Vec<String> {
    let url = format!("gurt://{}/sitemap.xml", domain);
    match client.fetch_with_retries(&url, 1).await {
        Ok(resp) if (200..300).contains(&resp.code) => {
            let body = String::from_utf8(resp.body).unwrap_or_default();
            parse_sitemap_xml(&body)
        }
        _ => Vec::new(),
    }
}

/// Reorder candidate URLs by prioritizing those present in the sitemap list.
/// URLs appearing in `sitemap_urls` are kept first (stable order), followed by others.
pub fn prioritize_with_sitemap(mut candidates: Vec<String>, sitemap_urls: &[String]) -> Vec<String> {
    if sitemap_urls.is_empty() || candidates.is_empty() { return candidates; }
    use std::collections::HashSet;
    let sm: HashSet<&str> = sitemap_urls.iter().map(|s| s.as_str()).collect();
    let mut a = Vec::with_capacity(candidates.len());
    let mut b = Vec::new();
    for u in candidates.drain(..) {
        if sm.contains(u.as_str()) { a.push(u); } else { b.push(u); }
    }
    a.extend(b);
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_sitemap() {
        let xml = r#"<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">
  <url><loc>gurt://example.real/</loc></url>
  <url>
    <loc>gurt://example.real/about</loc>
  </url>
  <url>
    <loc>
      gurt://example.real/blog/1
    </loc>
  </url>
</urlset>"#;
        let urls = parse_sitemap_xml(xml);
        assert_eq!(urls.len(), 3);
        assert_eq!(urls[0], "gurt://example.real/");
        assert_eq!(urls[1], "gurt://example.real/about");
        assert_eq!(urls[2], "gurt://example.real/blog/1");
    }

    #[test]
    fn prioritize_urls_with_sitemap() {
        let cand = vec![
            "gurt://example.real/x".to_string(),
            "gurt://example.real/".to_string(),
            "gurt://example.real/y".to_string(),
            "gurt://example.real/docs".to_string(),
        ];
        let sm = vec![
            "gurt://example.real/".to_string(),
            "gurt://example.real/docs".to_string(),
        ];
        let out = prioritize_with_sitemap(cand, &sm);
        assert_eq!(out[0], "gurt://example.real/");
        assert_eq!(out[1], "gurt://example.real/docs");
    }
}
