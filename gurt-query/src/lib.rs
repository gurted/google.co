#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryFilters {
    pub site: Option<String>,
    pub filetype: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuery {
    pub terms: Vec<String>,
    pub filters: QueryFilters,
}

impl Default for QueryFilters {
    fn default() -> Self {
        Self {
            site: None,
            filetype: None,
        }
    }
}

/// Parse a raw query string into free-text terms and supported filters.
/// Supported filters: `site:<domain>`, `filetype:<ext>` (case-insensitive keys).
/// - Domains and filetypes are lowercased and stripped of surrounding quotes.
/// - Unknown tokens are treated as free-text terms.
/// - Multiple occurrences: the last one wins.
pub fn parse_query(input: &str) -> ParsedQuery {
    let mut terms: Vec<String> = Vec::new();
    let mut site: Option<String> = None;
    let mut filetype: Option<String> = None;

    for raw in input.split_whitespace() {
        if let Some((k, v)) = raw.split_once(':') {
            match k.to_ascii_lowercase().as_str() {
                "site" => {
                    let v = strip_quotes(v).to_ascii_lowercase();
                    if !v.is_empty() {
                        site = Some(v);
                    }
                    continue;
                }
                "filetype" => {
                    let v = strip_quotes(v).to_ascii_lowercase();
                    if !v.is_empty() {
                        filetype = Some(v);
                    }
                    continue;
                }
                _ => {}
            }
        }
        // treat as a term
        if !raw.is_empty() {
            terms.push(raw.to_string());
        }
    }

    ParsedQuery {
        terms,
        filters: QueryFilters { site, filetype },
    }
}

fn strip_quotes(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &s[1..s.len() - 1];
        }
    }
    s
}
