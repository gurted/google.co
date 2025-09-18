use std::path::PathBuf;

use gurt_api::server::{init_tls, ServerConfig};

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
}

#[test]
fn loads_valid_pem_files() {
    let cert = fixture("cert.pem");
    let key = fixture("key.pem");
    let cfg = ServerConfig::new(cert, key);
    let material = init_tls(&cfg).expect("should load tls materials");
    assert!(material.is_pem());
}

#[test]
fn errors_on_missing_files() {
    let cfg = ServerConfig::new("tests/fixtures/missing-cert.pem", "tests/fixtures/key.pem");
    let err = init_tls(&cfg).expect_err("should error on missing cert");
    let msg = format!("{}", err);
    assert!(msg.contains("file not found"));
}

#[test]
fn errors_on_invalid_pem() {
    let cert = fixture("invalid-cert.pem");
    let key = fixture("invalid-key.pem");
    let cfg = ServerConfig::new(cert, key);
    let err = init_tls(&cfg).expect_err("should error on invalid pem");
    let msg = format!("{}", err);
    assert!(msg.contains("invalid pem"));
}
