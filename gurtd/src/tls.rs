use anyhow::{anyhow, Result, Context};
use rustls::{Certificate, PrivateKey, ServerConfig};
use std::{fs::File, io::BufReader, sync::Arc};
use tokio_rustls::TlsAcceptor;
use sha2::{Sha256, Digest};

pub struct TlsConfig {
    cfg: Arc<ServerConfig>,
}

impl TlsConfig {
    pub fn load(cert_path: &str, key_path: &str) -> Result<Self> {
        let certs = load_certs(cert_path)?;
        let key = load_key(key_path)?;

        let mut config = ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        // Enforce ALPN (TLS version checked post-accept)
        config.alpn_protocols = vec![b"GURT/1.0".to_vec()];

        Ok(Self { cfg: Arc::new(config) })
    }

    pub fn into_acceptor(self) -> TlsAcceptor {
        TlsAcceptor::from(self.cfg)
    }
}

fn load_certs(path: &str) -> Result<Vec<Certificate>> {
    let f = File::open(path).with_context(|| format!("opening certificate '{path}'"))?;
    let mut reader = BufReader::new(f);
    let certs = rustls_pemfile::certs(&mut reader)
        .map_err(|_| anyhow!("invalid certs"))?
        .into_iter()
        .map(Certificate)
        .collect::<Vec<_>>();
    if certs.is_empty() {
        return Err(anyhow!("no certificates found in {path}"));
    }
    // Log basic info and fingerprint of first cert for debugging
    if let Some(Certificate(first)) = certs.get(0) {
        let mut hasher = Sha256::new();
        hasher.update(first);
        let digest = hasher.finalize();
        let fp = digest.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(":");
        eprintln!(
            "[tls] loaded certificate chain (n={}) from {path}\n  leaf_sha256: {}",
            certs.len(),
            fp
        );
    }
    Ok(certs)
}

fn load_key(path: &str) -> Result<PrivateKey> {
    let f = File::open(path).with_context(|| format!("opening private key '{path}'"))?;
    let mut reader = BufReader::new(f);
    // Try PKCS8 first
    if let Ok(mut keys) = rustls_pemfile::pkcs8_private_keys(&mut reader) {
        if let Some(k) = keys.pop() { return Ok(PrivateKey(k)); }
    }
    // Fallback RSA
    let f = File::open(path).with_context(|| format!("opening private key '{path}'"))?;
    let mut reader = BufReader::new(f);
    if let Ok(mut keys) = rustls_pemfile::rsa_private_keys(&mut reader) {
        if let Some(k) = keys.pop() { return Ok(PrivateKey(k)); }
    }
    Err(anyhow!("no valid private key in {path}"))
}
