pub mod tls;
pub mod gurt;
pub mod status;
pub mod limits;
pub mod request;

pub mod server {
    use std::path::PathBuf;

    use crate::tls::{TlsMaterial, TlsResult};

    #[derive(Debug, Clone)]
    pub struct ServerConfig {
        pub cert_path: PathBuf,
        pub key_path: PathBuf,
    }

    impl ServerConfig {
        pub fn new(cert_path: impl Into<PathBuf>, key_path: impl Into<PathBuf>) -> Self {
            Self { cert_path: cert_path.into(), key_path: key_path.into() }
        }
    }

    pub fn init_tls(config: &ServerConfig) -> TlsResult<TlsMaterial> {
        TlsMaterial::from_files(&config.cert_path, &config.key_path)
    }
}
