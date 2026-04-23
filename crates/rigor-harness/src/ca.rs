use std::sync::Arc;
use anyhow::{Context, Result};
use rustls::ServerConfig;

/// Ephemeral test CA for TLS testing.
///
/// Follows the same `rcgen` pattern as production `RigorCA` in `daemon/tls.rs`
/// but never persists to disk. The CA cert and key live only in memory.
pub struct TestCA {
    ca_key: rcgen::KeyPair,
    ca_cert_signed: rcgen::Certificate,
    ca_cert_pem: String,
}

impl TestCA {
    /// Create a new ephemeral test CA.
    ///
    /// Installs the ring crypto provider idempotently (safe to call from
    /// multiple tests in parallel).
    pub fn new() -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();

        let mut ca_params = rcgen::CertificateParams::default();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "rigor-test-ca".to_string());
        ca_params
            .distinguished_name
            .push(rcgen::DnType::OrganizationName, "rigor-test".to_string());
        ca_params.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
        ];

        let ca_key = rcgen::KeyPair::generate().context("Failed to generate CA key")?;
        let ca_cert_signed = ca_params
            .self_signed(&ca_key)
            .context("Failed to self-sign CA cert")?;

        let ca_cert_pem = ca_cert_signed.pem();

        Ok(Self {
            ca_key,
            ca_cert_signed,
            ca_cert_pem,
        })
    }

    /// Build a `ServerConfig` presenting a cert for `hostname` signed by this CA.
    ///
    /// Follows the exact pattern from `daemon/tls.rs` lines 129-168.
    pub fn server_config_for_host(&self, hostname: &str) -> Result<Arc<ServerConfig>> {
        let mut params = rcgen::CertificateParams::new(vec![hostname.to_string()])
            .context("Failed to create cert params")?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, hostname.to_string());
        params
            .distinguished_name
            .push(rcgen::DnType::OrganizationName, "rigor-test".to_string());

        let host_key = rcgen::KeyPair::generate().context("Failed to generate host key")?;

        let host_cert = params
            .signed_by(&host_key, &self.ca_cert_signed, &self.ca_key)
            .context("Failed to sign host cert with CA")?;

        let host_cert_der = host_cert.der().clone();
        let ca_cert_der = self.ca_cert_signed.der().clone();
        let host_key_der = host_key.serialize_der();

        let certs = vec![
            rustls::pki_types::CertificateDer::from(host_cert_der.to_vec()),
            rustls::pki_types::CertificateDer::from(ca_cert_der.to_vec()),
        ];
        let key = rustls::pki_types::PrivateKeyDer::try_from(host_key_der)
            .map_err(|e| anyhow::anyhow!("failed to parse host key: {}", e))?;

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        Ok(Arc::new(config))
    }

    /// Build a `rustls::ClientConfig` that trusts only this test CA.
    pub fn client_config(&self) -> rustls::ClientConfig {
        let mut root_store = rustls::RootCertStore::empty();
        let ca_der = self.ca_cert_signed.der().clone();
        root_store
            .add(rustls::pki_types::CertificateDer::from(ca_der.to_vec()))
            .expect("add test CA to root store");

        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth()
    }

    /// Build a `reqwest::Client` that trusts this test CA.
    pub fn reqwest_client(&self) -> reqwest::Client {
        let cert = reqwest::tls::Certificate::from_pem(self.ca_cert_pem.as_bytes())
            .expect("parse test CA PEM for reqwest");

        reqwest::Client::builder()
            .add_root_certificate(cert)
            .build()
            .expect("build reqwest client with test CA")
    }

    /// Return the CA certificate in PEM format.
    pub fn ca_cert_pem(&self) -> &str {
        &self.ca_cert_pem
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ca_creation() {
        let ca = TestCA::new().expect("TestCA::new() should not fail");
        assert!(!ca.ca_cert_pem().is_empty());
    }

    #[test]
    fn test_server_config_for_host() {
        let ca = TestCA::new().unwrap();
        let config = ca.server_config_for_host("example.com");
        assert!(config.is_ok(), "server_config_for_host should succeed");
    }

    #[test]
    fn test_reqwest_client() {
        let ca = TestCA::new().unwrap();
        let _client = ca.reqwest_client();
    }

    #[test]
    fn test_double_ca_creation() {
        let _ca1 = TestCA::new().expect("first TestCA");
        let _ca2 = TestCA::new().expect("second TestCA (idempotent crypto provider)");
    }
}
