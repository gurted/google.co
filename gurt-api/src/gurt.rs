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
        reason: "Switching Protocols",
        headers: vec![
            ("Connection".to_string(), "Upgrade".to_string()),
            ("Upgrade".to_string(), "GURT/1.0".to_string()),
        ],
    }
}

