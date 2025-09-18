use gurtd::index::tantivy::TantivyIndexEngine;
use gurtd::index::IndexDocument;
use gurtd::index::IndexEngine;
use std::path::PathBuf;

fn tempdir() -> PathBuf {
    let mut p = std::env::temp_dir();
    let uniq = format!("gurtd-tantivy-{}-{}", std::process::id(), rand_suffix());
    p.push(uniq);
    p
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", ns)
}

#[test]
fn open_create_commit_refresh_updates_searcher_docs() {
    let dir = tempdir();
    let engine = TantivyIndexEngine::open_or_create_in_dir(&dir).expect("open/create index");
    assert_eq!(engine.num_docs(), 0);

    let doc = IndexDocument {
        url: "gurt://example.real/hello".into(),
        domain: "example.real".into(),
        title: "Hello".into(),
        content: "Hello world content".into(),
        fetch_time: 1_700_000_000,
        language: "en".into(),
        render_mode: "static".into(),
    };
    engine.add(doc).expect("add doc");
    engine.commit().expect("commit");
    engine.refresh().expect("refresh");

    assert_eq!(engine.num_docs(), 1);
}
