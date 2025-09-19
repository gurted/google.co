use std::collections::HashMap;

/// Extract links from HTML by scanning for <a ... href="..."> occurrences.
/// - Only returns absolute gurt:// URLs; relative URLs are ignored for simplicity in v1.
pub fn extract_links(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = html.as_bytes();
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while let Some(href_rel) = lower[i..].find("href") {
        let href_pos = i + href_rel;
        // find '=' after href
        let eq_opt = lower[href_pos..].find('=');
        if eq_opt.is_none() {
            break;
        }
        let mut j = href_pos + eq_opt.unwrap() + 1;
        // skip whitespace
        while j < html.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= html.len() {
            break;
        }
        // Some inputs may include literal backslashes before quotes (e.g., raw strings); skip them
        while j < html.len() && bytes[j] == b'\\' {
            j += 1;
        }
        if j >= html.len() {
            break;
        }
        let quote = bytes[j];
        if quote == b'"' || quote == b'\'' {
            j += 1;
            let mut k = j;
            while k < html.len() && bytes[k] != quote {
                k += 1;
            }
            let mut url = &html[j..k.min(html.len())];
            if url.starts_with("gurt://") {
                // Tolerate stray trailing backslashes before closing quote (from raw strings)
                url = url.trim_end_matches('\\');
                out.push(url.to_string());
            }
            i = k.saturating_add(1);
        } else {
            // unquoted value
            let mut k = j;
            while k < html.len() && !bytes[k].is_ascii_whitespace() && bytes[k] != b'>' {
                k += 1;
            }
            let url = &html[j..k];
            if url.starts_with("gurt://") {
                out.push(url.trim_end_matches('\\').to_string());
            }
            i = k;
        }
    }
    out
}

/// Directed link graph using adjacency lists.
#[derive(Default, Debug, Clone)]
pub struct LinkGraph {
    pub edges: HashMap<String, Vec<String>>, // from -> [to]
}

impl LinkGraph {
    pub fn new() -> Self {
        Self {
            edges: HashMap::new(),
        }
    }

    pub fn add_edge(&mut self, from: &str, to: &str) {
        self.edges
            .entry(from.to_string())
            .or_default()
            .push(to.to_string());
        // ensure sink nodes appear as keys
        self.edges.entry(to.to_string()).or_default();
    }

    /// Compute a simple PageRank-like score with damping over N iterations.
    pub fn pagerank(&self, damping: f64, iters: usize) -> HashMap<String, f64> {
        let nodes: Vec<String> = self.edges.keys().cloned().collect();
        let n = nodes.len().max(1);
        let base = (1.0 - damping) / n as f64;
        let mut rank: HashMap<String, f64> =
            nodes.iter().map(|k| (k.clone(), 1.0 / n as f64)).collect();

        // Precompute out-degree
        let mut out_deg: HashMap<&str, usize> = HashMap::new();
        for (u, vs) in &self.edges {
            out_deg.insert(u.as_str(), vs.len().max(1));
        }

        for _ in 0..iters {
            // power-iteration
            let mut next = HashMap::with_capacity(rank.len());
            // initialize with base
            for k in &nodes {
                next.insert(k.clone(), base);
            }
            for (u, vs) in &self.edges {
                let ru = *rank.get(u).unwrap_or(&0.0);
                let share = damping * (ru / out_deg.get(u.as_str()).copied().unwrap_or(1) as f64);
                for v in vs {
                    *next.entry(v.clone()).or_insert(base) += share;
                }
            }
            rank = next;
        }
        rank
    }
}

/// Simple domain trust score informed by DNS CNAME chain depth.
/// - Depth 0..=5 accepted; deeper chains result in 0.0 trust.
/// - Trust decays with depth: trust = 1.0 / (1 + depth)
pub fn domain_trust_from_cname_depth(depth: usize) -> f64 {
    if depth > 5 {
        return 0.0;
    }
    1.0 / (1.0 + depth as f64)
}

/// Combine document authority (PageRank) with domain trust.
/// Final score = alpha * pr + (1-alpha) * domain_trust
pub fn combine_authority(pr: f64, domain_trust: f64, alpha: f64) -> f64 {
    let a = alpha.clamp(0.0, 1.0);
    a * pr + (1.0 - a) * domain_trust
}

/// In-memory per-document authority score store with simple JSON persistence.
#[derive(Default, Debug, Clone)]
pub struct AuthorityStore {
    map: HashMap<String, f32>,
}

impl AuthorityStore {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }
    pub fn set(&mut self, url: String, score: f32) {
        self.map.insert(url, score);
    }
    pub fn get(&self, url: &str) -> Option<f32> {
        self.map.get(url).copied()
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn to_json(&self) -> String {
        let mut items: Vec<(&String, &f32)> = self.map.iter().collect();
        items.sort_by(|a, b| a.0.cmp(b.0));
        let mut s = String::from("{\n");
        for (i, (k, v)) in items.iter().enumerate() {
            let comma = if i + 1 == items.len() { "" } else { "," };
            s.push_str(&format!("  \"{}\": {:.6}{}\n", k, v, comma));
        }
        s.push_str("}\n");
        s
    }

    pub fn from_json(s: &str) -> Self {
        let mut out = Self::new();
        for line in s.lines() {
            let line = line.trim();
            if line.starts_with('"') {
                if let Some((k, rest)) = line[1..].split_once('"') {
                    if let Some(colon) = rest.find(':') {
                        let val_str = rest[colon + 1..].trim().trim_end_matches(',');
                        if let Ok(val) = val_str.parse::<f32>() {
                            out.set(k.to_string(), val);
                        }
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_basic_links() {
        let html = r#"<a href=\"gurt://example.real/\">home</a> <a class=\"x\" href='gurt://a/b'>b</a> <a href=/rel>rel</a>"#;
        let links = extract_links(html);
        assert_eq!(links, vec!["gurt://example.real/", "gurt://a/b"]);
    }

    #[test]
    fn pagerank_small_graph() {
        let mut g = LinkGraph::new();
        g.add_edge("A", "B");
        g.add_edge("B", "C");
        g.add_edge("C", "A"); // cycle A->B->C->A
        let pr = g.pagerank(0.85, 20);
        assert_eq!(pr.len(), 3);
        // roughly equal for a simple cycle
        let a = pr.get("A").unwrap();
        let b = pr.get("B").unwrap();
        let c = pr.get("C").unwrap();
        assert!((a - b).abs() < 1e-6 && (b - c).abs() < 1e-6);
    }

    #[test]
    fn trust_from_cname_depth() {
        assert_eq!(domain_trust_from_cname_depth(0), 1.0);
        assert!(domain_trust_from_cname_depth(3) > domain_trust_from_cname_depth(4));
        assert_eq!(domain_trust_from_cname_depth(6), 0.0);
    }

    #[test]
    fn authority_store_json_roundtrip() {
        let mut s = AuthorityStore::new();
        s.set("gurt://a".into(), 0.12);
        s.set("gurt://b".into(), 0.34);
        let j = s.to_json();
        let r = AuthorityStore::from_json(&j);
        assert_eq!(r.len(), 2);
        assert!((r.get("gurt://a").unwrap() - 0.12).abs() < 1e-6);
    }
}
