use gurtd::query::{parse_query, ParsedQuery, QueryFilters};

#[test]
fn parses_supported_filters_and_terms() {
    let pq = parse_query("rust site:Example.COM filetype:PDF tutorial");
    assert_eq!(pq.filters.site.as_deref(), Some("example.com"));
    assert_eq!(pq.filters.filetype.as_deref(), Some("pdf"));
    assert_eq!(pq.terms, vec!["rust", "tutorial"]);
}

#[test]
fn strips_quotes_and_normalizes_values() {
    let pq = parse_query("\"multi word\" site:'Docs.Example.COM' filetype:\"Pdf\"");
    assert_eq!(pq.filters.site.as_deref(), Some("docs.example.com"));
    assert_eq!(pq.filters.filetype.as_deref(), Some("pdf"));
    // The quoted multi-word token is not grouped by our simple parser; asserts tokenization
    assert_eq!(pq.terms, vec!["\"multi", "word\""]);
}

#[test]
fn last_occurrence_wins_for_duplicate_filters() {
    let pq = parse_query("site:a.com site:b.com filetype:html filetype:pdf x");
    assert_eq!(pq.filters.site.as_deref(), Some("b.com"));
    assert_eq!(pq.filters.filetype.as_deref(), Some("pdf"));
    assert_eq!(pq.terms, vec!["x"]);
}

#[test]
fn unsupported_filter_tokens_become_terms() {
    let pq = parse_query("lang:en tag:news rust");
    // Unsupported filters should be treated as free-text tokens
    assert_eq!(pq.filters.site, None);
    assert_eq!(pq.filters.filetype, None);
    assert_eq!(pq.terms, vec!["lang:en", "tag:news", "rust"]);
}

#[test]
fn empty_or_missing_filter_values_are_ignored() {
    let pq = parse_query("site: filetype:  rust");
    assert_eq!(pq.filters.site, None);
    assert_eq!(pq.filters.filetype, None);
    assert_eq!(pq.terms, vec!["rust"]);
}

