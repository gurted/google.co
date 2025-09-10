use std::path::PathBuf;
use gurtd::index::tantivy::TantivyIndexEngine;
use gurtd::index::{IndexDocument, IndexEngine};
use gurtd::query::{ParsedQuery, QueryFilters};

fn tempdir() -> PathBuf {
    let mut p = std::env::temp_dir();
    let uniq = format!("gurtd-bm25-{}-{}", std::process::id(), rand_suffix());
    p.push(uniq);
    p
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ns = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    format!("{:x}", ns)
}

#[test]
fn stopwords_are_removed_in_query() {
    let dir = tempdir();
    let engine = TantivyIndexEngine::open_or_create_in_dir(&dir).expect("open/create index");

    // Index a doc containing a meaningful term plus some stopwords.
    engine.add(IndexDocument {
        url: "gurt://example.real/doc1".into(),
        domain: "example.real".into(),
        title: "The Rust Book".into(),
        content: "The and of rust".into(),
        fetch_time: 1_700_000_000,
        language: "en".into(),
        render_mode: "static".into(),
    }).unwrap();
    engine.commit().unwrap();
    engine.refresh().unwrap();

    // Query mixes upper-case and stopwords.
    let pq = ParsedQuery { terms: vec!["THE".into(), "and".into(), "RUST".into(), "of".into()], filters: QueryFilters::default() };
    let hits = engine.search(&pq, 1, 10).expect("search ok");
    assert!(!hits.is_empty(), "should match after removing stopwords");
}

#[test]
fn bm25_prefers_higher_tf() {
    let dir = tempdir();
    let engine = TantivyIndexEngine::open_or_create_in_dir(&dir).expect("open/create index");

    // Doc with higher term frequency for 'rust'
    engine.add(IndexDocument {
        url: "gurt://example.real/doc_tf2".into(),
        domain: "example.real".into(),
        title: "rust rust".into(),
        content: "rust language".into(),
        fetch_time: 1_700_000_001,
        language: "en".into(),
        render_mode: "static".into(),
    }).unwrap();

    // Doc with lower term frequency
    engine.add(IndexDocument {
        url: "gurt://example.real/doc_tf1".into(),
        domain: "example.real".into(),
        title: "rust".into(),
        content: "programming".into(),
        fetch_time: 1_700_000_002,
        language: "en".into(),
        render_mode: "static".into(),
    }).unwrap();

    engine.commit().unwrap();
    engine.refresh().unwrap();

    let pq = ParsedQuery { terms: vec!["rust".into()], filters: QueryFilters::default() };
    let hits = engine.search(&pq, 1, 10).expect("search ok");
    assert!(hits.len() >= 2);
    // Expect first score >= second due to higher term frequency
    assert!(hits[0].score >= hits[1].score, "expected top score >= second: {:?}", hits);
}
