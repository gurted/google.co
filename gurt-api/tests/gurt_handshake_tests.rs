use gurt_api::gurt::perform_handshake;

#[test]
fn handshake_returns_101_with_required_headers() {
    let hs = perform_handshake();
    assert_eq!(hs.status, 101);
    assert_eq!(hs.reason, "Switching Protocols");

    let upgrade = hs.header("Upgrade").unwrap_or("");
    let conn = hs.header("Connection").unwrap_or("");
    assert_eq!(upgrade, "GURT/1.0");
    assert_eq!(conn, "Upgrade");
}

