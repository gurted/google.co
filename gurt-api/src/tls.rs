use std::{fs, path::Path};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TlsError {
    #[error("file not found: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid pem format")]
    InvalidPem,
}

pub type TlsResult<T> = Result<T, TlsError>;

#[derive(Debug, Clone)]
pub struct TlsMaterial {
    pub cert_pem: String,
    pub key_pem: String,
}

impl TlsMaterial {
    pub fn from_files(cert_path: &Path, key_path: &Path) -> TlsResult<Self> {
        if !cert_path.exists() {
            return Err(TlsError::NotFound(cert_path.display().to_string()));
        }
        if !key_path.exists() {
            return Err(TlsError::NotFound(key_path.display().to_string()));
        }

        let cert_pem = fs::read_to_string(cert_path)?;
        let key_pem = fs::read_to_string(key_path)?;

        let material = Self { cert_pem, key_pem };
        if !material.is_pem() {
            return Err(TlsError::InvalidPem);
        }
        Ok(material)
    }

    pub fn is_pem(&self) -> bool {
        self.cert_pem.contains("-----BEGIN CERTIFICATE-----")
            && (self.key_pem.contains("-----BEGIN PRIVATE KEY-----")
                || self.key_pem.contains("-----BEGIN RSA PRIVATE KEY-----"))
    }
}
