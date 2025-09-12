use std::cmp::Ordering;
use std::time::Duration;

/// Parsed robots.txt rules for a single domain.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RobotsTxt {
    /// Rules grouped by user-agent token (lowercased). `*` is the wildcard group.
    groups: Vec<AgentGroup>,
}

#[derive(Debug, Clone, PartialEq)]
struct AgentGroup {
    agent: String, // lowercased agent token, e.g., "*" or "gurtbot"
    allow: Vec<String>,
    disallow: Vec<String>,
    crawl_delay: Option<Duration>,
}

impl RobotsTxt {
    /// Parse a robots.txt document with basic HTTP-like semantics.
    /// - Supports User-agent, Allow, Disallow, Crawl-delay.
    /// - Path matching is prefix-based. Longest rule wins; ties prefer Allow.
    /// - User-agent matches are case-insensitive and prefer the longest matching agent; fallback to `*`.
    pub fn parse(input: &str) -> Self {
        let mut groups: Vec<AgentGroup> = Vec::new();
        let mut current_agents: Vec<String> = Vec::new();

        for raw_line in input.lines() {
            let line = raw_line.trim();
            if line.is_empty() { continue; }
            if line.starts_with('#') { continue; }
            let Some((k, v)) = line.split_once(':') else { continue }; // ignore invalid lines
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim();
            match key.as_str() {
                "user-agent" => {
                    let agent = val.to_ascii_lowercase();
                    if current_agents.is_empty() { current_agents.clear(); }
                    current_agents.push(agent);
                    // ensure groups exist
                    for a in &current_agents {
                        get_or_create_group_index(&mut groups, a);
                    }
                }
                "allow" => {
                    if current_agents.is_empty() {
                        current_agents.push("*".to_string());
                    }
                    for a in &current_agents {
                        let idx = get_or_create_group_index(&mut groups, a);
                        groups[idx].allow.push(val.to_string());
                    }
                }
                "disallow" => {
                    if current_agents.is_empty() {
                        current_agents.push("*".to_string());
                    }
                    for a in &current_agents {
                        let idx = get_or_create_group_index(&mut groups, a);
                        groups[idx].disallow.push(val.to_string());
                    }
                }
                "crawl-delay" => {
                    let delay = parse_crawl_delay(val);
                    if current_agents.is_empty() {
                        current_agents.push("*".to_string());
                    }
                    for a in &current_agents {
                        let idx = get_or_create_group_index(&mut groups, a);
                        groups[idx].crawl_delay = delay;
                    }
                }
                _ => {}
            }
        }

        // If no groups defined at all, create a default wildcard
        if groups.is_empty() {
            groups.push(AgentGroup { agent: "*".to_string(), allow: vec![], disallow: vec![], crawl_delay: None });
        }
        Self { groups }
    }

    /// Determine whether a path is allowed for the given user-agent token.
    pub fn is_allowed(&self, user_agent: &str, path: &str) -> bool {
        // Choose applicable group: exact/longest match of agent token; fallback to '*'
        let ua = user_agent.to_ascii_lowercase();
        let mut best: Option<&AgentGroup> = None;
        for g in &self.groups {
            if g.agent == "*" || ua.contains(&g.agent) {
                best = match best {
                    None => Some(g),
                    Some(prev) => {
                        // prefer longer agent token (more specific)
                        if g.agent.len() > prev.agent.len() { Some(g) } else { Some(prev) }
                    }
                };
            }
        }
        let group = best.unwrap_or(&self.groups[0]);
        match most_specific_rule(group, path) {
            None => true,         // default allow
            Some(Rule::Allow(_)) => true,
            Some(Rule::Disallow(_)) => false,
        }
    }

    /// Get crawl-delay directive for the given user-agent, if any.
    pub fn crawl_delay(&self, user_agent: &str) -> Option<Duration> {
        let ua = user_agent.to_ascii_lowercase();
        let mut best: Option<&AgentGroup> = None;
        for g in &self.groups {
            if g.agent == "*" || ua.contains(&g.agent) {
                best = match best {
                    None => Some(g),
                    Some(prev) => if g.agent.len() > prev.agent.len() { Some(g) } else { Some(prev) },
                };
            }
        }
        best.and_then(|g| g.crawl_delay)
    }
}

fn parse_crawl_delay(s: &str) -> Option<Duration> {
    // supports integer or float seconds
    let sv = s.trim();
    if sv.is_empty() { return None; }
    if let Ok(n) = sv.parse::<u64>() { return Some(Duration::from_secs(n)); }
    if let Ok(f) = sv.parse::<f64>() { return Some(Duration::from_secs_f64(f.max(0.0))); }
    None
}

#[derive(Debug, Clone, PartialEq)]
enum Rule {
    Allow(String),
    Disallow(String),
}

fn most_specific_rule(group: &AgentGroup, path: &str) -> Option<Rule> {
    let mut best: Option<Rule> = None;
    let test = |pattern: &str, kind: fn(String) -> Rule, best: &mut Option<Rule>| {
        if pattern.is_empty() { return; }
        // Basic prefix match. Standard allows wildcards; out of scope for v1.
        if path.starts_with(pattern) {
            match best {
                None => { *best = Some(kind(pattern.to_string())); }
                Some(prev) => {
                    let prev_len = match prev { Rule::Allow(s) | Rule::Disallow(s) => s.len() };
                    match pattern.len().cmp(&prev_len) {
                        Ordering::Greater => *best = Some(kind(pattern.to_string())),
                        Ordering::Equal => {
                            // tie-breaker: Allow wins over Disallow
                            if matches!(prev, Rule::Disallow(_)) && matches!(kind(String::new()), Rule::Allow(_)) {
                                *best = Some(kind(pattern.to_string()));
                            }
                        }
                        Ordering::Less => {}
                    }
                }
            }
        }
    };

    for a in &group.allow { test(a, Rule::Allow, &mut best); }
    for d in &group.disallow { test(d, Rule::Disallow, &mut best); }
    best
}

fn get_or_create_group_index(groups: &mut Vec<AgentGroup>, agent: &str) -> usize {
    if let Some((i, _)) = groups.iter().enumerate().find(|(_, g)| g.agent == agent) {
        return i;
    }
    groups.push(AgentGroup { agent: agent.to_string(), allow: vec![], disallow: vec![], crawl_delay: None });
    groups.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_match_basic_rules() {
        let txt = "\
User-agent: *\n\
Disallow: /private\n\
Allow: /private/open\n\
Crawl-delay: 2.5\n\
";
        let r = RobotsTxt::parse(txt);
        assert!(r.is_allowed("gurtbot", "/"));
        assert!(!r.is_allowed("gurtbot", "/private/index.html"));
        assert!(r.is_allowed("gurtbot", "/private/open/file"));
        let d = r.crawl_delay("gurtbot").unwrap();
        assert!(d.as_secs_f64() > 2.4 && d.as_secs_f64() < 2.6);
    }

    #[test]
    fn agent_specificity() {
        let txt = "\
User-agent: gurt\n\
Disallow: /blocked\n\
\n\
User-agent: *\n\
Allow: /\n\
";
        let r = RobotsTxt::parse(txt);
        assert!(!r.is_allowed("gurtbot", "/blocked/page"));
        assert!(r.is_allowed("otherbot", "/blocked/page"));
    }
}

impl RobotsTxt {
    /// Fetch and parse robots.txt for a domain using the provided client.
    /// Returns None if missing (non-2xx) or on network/protocol errors.
    pub async fn fetch_for_domain(client: &crate::crawler::client::GurtClient, domain: &str) -> Option<Self> {
        let url = format!("gurt://{}/robots.txt", domain);
        match client.fetch_with_retries(&url, 1).await {
            Ok(resp) if (200..300).contains(&resp.code) => {
                let body = String::from_utf8(resp.body).unwrap_or_default();
                Some(Self::parse(&body))
            }
            _ => None,
        }
    }
}

/// Determine if a URL path is allowed for a given domain and user-agent.
/// - If robots is None (missing/unfetchable), default allow per requirements.
pub fn is_allowed_with_robots(robots: Option<&RobotsTxt>, user_agent: &str, path: &str) -> bool {
    match robots { Some(r) => r.is_allowed(user_agent, path), None => true }
}
