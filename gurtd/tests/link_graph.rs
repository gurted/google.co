use gurtd::link::{extract_links, LinkGraph, domain_trust_from_cname_depth, combine_authority, AuthorityStore};

#[test]
fn extract_links_from_html() {
    let html = r#"
      <html><body>
        <a href="gurt://one/">one</a>
        <a href='gurt://two/a'>two</a>
        <a href=/relative>rel</a>
      </body></html>
    "#;
    let links = extract_links(html);
    assert_eq!(links, vec!["gurt://one/", "gurt://two/a"]);
}

#[test]
fn pagerank_cycle_is_balanced() {
    let mut g = LinkGraph::new();
    g.add_edge("A", "B");
    g.add_edge("B", "C");
    g.add_edge("C", "A");
    let pr = g.pagerank(0.85, 20);
    let a = pr.get("A").unwrap();
    let b = pr.get("B").unwrap();
    let c = pr.get("C").unwrap();
    assert!((a - b).abs() < 1e-6 && (b - c).abs() < 1e-6);
}

#[test]
fn trust_and_combine() {
    let t0 = domain_trust_from_cname_depth(0);
    let t4 = domain_trust_from_cname_depth(4);
    assert!(t0 > t4);
    assert_eq!(domain_trust_from_cname_depth(6), 0.0);
    let pr = 0.3;
    let combined = combine_authority(pr, t0, 0.5);
    assert!(combined >= pr && combined <= 1.0);
}

#[test]
fn authority_store_roundtrip() {
    let mut s = AuthorityStore::new();
    s.set("gurt://x".into(), 0.5);
    s.set("gurt://y".into(), 0.7);
    let j = s.to_json();
    let r = AuthorityStore::from_json(&j);
    assert_eq!(r.len(), 2);
    assert!((r.get("gurt://x").unwrap() - 0.5).abs() < 1e-6);
}

