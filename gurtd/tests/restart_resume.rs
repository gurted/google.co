use gurtd::index::tantivy::TantivyIndexEngine;
use gurtd::index::{IndexDocument, IndexEngine};
use std::path::PathBuf;

fn tempdir() -> PathBuf {
    let mut p = std::env::temp_dir();
    let uniq = format!("gurtd-restart-{}-{}", std::process::id(), rand_suffix());
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
fn disk_index_persists_across_reopen() {
    let dir = tempdir();

    // first: create, add doc, commit+refresh
    {
        let engine = TantivyIndexEngine::open_or_create_in_dir(&dir).expect("open/create");
        assert_eq!(engine.num_docs(), 0);

        engine
            .add(IndexDocument {
                url: "gurt://example.real/persist".into(),
                domain: "example.real".into(),
                title: "Persist Me".into(),
                content: "Content survives restarts".into(),
                fetch_time: 1_700_000_123,
                language: "en".into(),
                render_mode: "static".into(),
            })
            .expect("add");
        engine.commit().expect("commit");
        engine.refresh().expect("refresh");
        assert_eq!(engine.num_docs(), 1);
    }

    // second: reopen same directory, verify doc count is still visible
    {
        let engine = TantivyIndexEngine::open_or_create_in_dir(&dir).expect("reopen");
        assert_eq!(engine.num_docs(), 1);
    }
}