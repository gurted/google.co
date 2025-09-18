use std::time::Duration;

/// Reasons that mark a page as a candidate for dynamic render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicReason {
    LuaScriptTag,
    NetworkFetch,
}

/// Render configuration for the selective render-once pipeline.
#[derive(Debug, Clone)]
pub struct RenderConfig {
    pub time_budget: Duration,
    /// Internal/testing knob to simulate render work cost.
    pub simulated_cost: Option<Duration>,
}

impl RenderConfig {
    pub fn with_budget_ms(ms: u64) -> Self {
        Self {
            time_budget: Duration::from_millis(ms),
            simulated_cost: None,
        }
    }
}

/// Outcome of a render attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOutcome {
    pub content: String,
    pub render_mode: String, // "static" | "rendered"
    pub timed_out: bool,
    pub reason: Option<DynamicReason>,
}

/// Heuristics to detect dynamic pages that merit a render-once pass.
/// - Detects <script type="text/lua"> or other occurrences of "lua" in script tags.
/// - Detects occurrences of "network.fetch(".
pub fn detect_dynamic(html: &str) -> Option<DynamicReason> {
    let lower = html.to_ascii_lowercase();
    if lower.contains("<script") && lower.contains("lua") {
        return Some(DynamicReason::LuaScriptTag);
    }
    if lower.contains("network.fetch(") {
        return Some(DynamicReason::NetworkFetch);
    }
    None
}

/// Perform a render-once pipeline if heuristics indicate dynamic content.
/// This is a stub implementation: it removes <script>...</script> blocks and emits a
/// "rendered" marker if completed within `time_budget`. If the budget is exceeded,
/// returns the original content as static with `timed_out = true`.
pub async fn render_once(html: &str, cfg: &RenderConfig) -> RenderOutcome {
    let reason = detect_dynamic(html);
    if reason.is_none() {
        return RenderOutcome {
            content: html.to_string(),
            render_mode: "static".into(),
            timed_out: false,
            reason: None,
        };
    }

    // Enforce budget using a simulated cost (tests) or minimal yield
    if let Some(cost) = cfg.simulated_cost {
        if cost > cfg.time_budget {
            // Exceeds budget: return static fallback
            return RenderOutcome {
                content: html.to_string(),
                render_mode: "static".into(),
                timed_out: true,
                reason,
            };
        }
        // sleep to simulate work but within budget
        if !cost.is_zero() {
            tokio::time::sleep(cost).await;
        }
    } else {
        // Yield once to simulate minimal work without exceeding budget
        tokio::task::yield_now().await;
    }

    // Perform a simple transformation: strip <script> blocks and append a marker
    let mut out = html.to_string();
    out = strip_script_blocks(&out);
    if !out.contains("<!-- rendered -->") {
        out.push_str("\n<!-- rendered -->");
    }

    RenderOutcome {
        content: out,
        render_mode: "rendered".into(),
        timed_out: false,
        reason,
    }
}

fn strip_script_blocks(input: &str) -> String {
    // TODO: this is a naive scan; swap for a real HTML tokenizer when script edge cases start to bite.
    let mut out = String::new();
    let mut rest = input;
    loop {
        let lower = rest.to_ascii_lowercase();
        if let Some(start) = lower.find("<script") {
            out.push_str(&rest[..start]);
            let after = &rest[start..];
            if let Some(close_pos) = lower[start..].find("</script>") {
                // skip the whole script segment
                let end = start + close_pos + "</script>".len();
                rest = &rest[end..];
                continue;
            } else {
                // No closing tag; drop remainder
                break;
            }
        } else {
            out.push_str(rest);
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristics_detect_lua_and_fetch() {
        assert!(matches!(
            detect_dynamic("<script type=\"text/lua\">print(1)</script>"),
            Some(DynamicReason::LuaScriptTag)
        ));
        assert!(matches!(
            detect_dynamic("<div>call network.fetch(\"/api\")</div>"),
            Some(DynamicReason::NetworkFetch)
        ));
        assert!(detect_dynamic("<div>static</div>").is_none());
    }

    #[tokio::test]
    async fn render_static_passthrough() {
        let html = "<html><body>static</body></html>";
        let cfg = RenderConfig::with_budget_ms(10);
        let out = render_once(html, &cfg).await;
        assert_eq!(out.render_mode, "static");
        assert_eq!(out.content, html);
        assert!(!out.timed_out);
        assert!(out.reason.is_none());
    }

    #[tokio::test]
    async fn render_dynamic_success() {
        let html = "<html><body><script type=\"text/lua\">print(1)</script><div id=\"app\"></div></body></html>";
        let mut cfg = RenderConfig::with_budget_ms(50);
        cfg.simulated_cost = Some(Duration::from_millis(5));
        let out = render_once(html, &cfg).await;
        assert_eq!(out.render_mode, "rendered");
        assert!(!out.content.contains("<script"));
        assert!(out.content.contains("<!-- rendered -->"));
        assert!(!out.timed_out);
        assert!(out.reason.is_some());
    }

    #[tokio::test]
    async fn render_dynamic_timeout_fallback() {
        let html = "<html><body>call network.fetch(\"/api\")</body></html>";
        let mut cfg = RenderConfig::with_budget_ms(10);
        cfg.simulated_cost = Some(Duration::from_millis(25));
        let out = render_once(html, &cfg).await;
        assert_eq!(out.render_mode, "static");
        assert!(out.timed_out);
        assert_eq!(out.content, html);
    }
}
