use gurt_api::gurt::perform_handshake;

#[test]
fn handshake_returns_101_with_required_headers() {
    let hs = perform_handshake();
    assert_eq!(hs.status, 101);
    assert_eq!(hs.reason, "SWITCHING_PROTOCOLS");

    assert_eq!(hs.header("gurt-version").unwrap_or(""), "1.0.0");
    assert_eq!(hs.header("encryption").unwrap_or(""), "TLS/1.3");
    assert_eq!(hs.header("alpn").unwrap_or(""), "GURT/1.0");
    assert_eq!(hs.header("server").unwrap_or(""), "GURT/1.0.0");
}
