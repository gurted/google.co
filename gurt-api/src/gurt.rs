#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandshakeResponse {
    pub status: u16,
    pub reason: &'static str,
    pub headers: Vec<(String, String)>,
}

impl HandshakeResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub fn perform_handshake() -> HandshakeResponse {
    HandshakeResponse {
        status: 101,
        reason: "SWITCHING_PROTOCOLS",
        headers: vec![
            ("gurt-version".to_string(), "1.0.0".to_string()),
            ("encryption".to_string(), "TLS/1.3".to_string()),
            ("alpn".to_string(), "GURT/1.0".to_string()),
            ("server".to_string(), "GURT/1.0.0".to_string()),
        ],
    }
}
