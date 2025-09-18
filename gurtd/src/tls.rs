use anyhow::{anyhow, Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use sha2::{Digest, Sha256};
use std::{fs::File, io::BufReader, sync::Arc};
use tokio_rustls::TlsAcceptor;

pub struct TlsConfig {
    cfg: Arc<ServerConfig>,
}

impl TlsConfig {
    pub fn load(cert_path: &str, key_path: &str) -> Result<Self> {
        let certs = load_certs(cert_path)?;
        let key = load_key(key_path)?;

        let mut config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        // Enforce ALPN (TLS version checked post-accept)
        config.alpn_protocols = vec![b"GURT/1.0".to_vec()];

        Ok(Self {
            cfg: Arc::new(config),
        })
    }

    pub fn into_acceptor(self) -> TlsAcceptor {
        TlsAcceptor::from(self.cfg)
    }
}

fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let f = File::open(path).with_context(|| format!("opening certificate '{path}'"))?;
    let mut reader = BufReader::new(f);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| anyhow!("invalid certs"))?;
    if certs.is_empty() {
        return Err(anyhow!("no certificates found in {path}"));
    }
    // Log basic info and fingerprint of first cert for debugging
    if let Some(first) = certs.get(0) {
        let mut hasher = Sha256::new();
        hasher.update(first.as_ref());
        let digest = hasher.finalize();
        let fp = digest
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(":");
        eprintln!(
            "[tls] loaded certificate chain (n={}) from {path}\n  leaf_sha256: {}",
            certs.len(),
            fp
        );
    }
    Ok(certs)
}

fn load_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let f = File::open(path).with_context(|| format!("opening private key '{path}'"))?;
    let mut reader = BufReader::new(f);
    // Iterate PEM items until we find a supported key
    while let Some(item) =
        rustls_pemfile::read_one(&mut reader).map_err(|_| anyhow!("invalid pem"))?
    {
        match item {
            rustls_pemfile::Item::Pkcs8Key(k) => return Ok(PrivateKeyDer::from(k)),
            rustls_pemfile::Item::Pkcs1Key(k) => return Ok(PrivateKeyDer::from(k)),
            rustls_pemfile::Item::Sec1Key(k) => return Ok(PrivateKeyDer::from(k)),
            _ => {}
        }
    }
    Err(anyhow!("no valid private key in {path}"))
}
