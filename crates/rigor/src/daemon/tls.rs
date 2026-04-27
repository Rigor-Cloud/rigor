//! TLS Certificate Authority for rigor MITM.
//!
//! Follows the mitmproxy/Charles Proxy/Burp Suite pattern:
//! 1. Generate a CA cert once, persist at ~/.rigor/ca.pem + ca-key.pem
//! 2. On each CONNECT, generate a per-host cert signed by the CA
//! 3. If the CA is installed in the OS trust store, ALL apps trust the MITM cert
//!    (not just Node/Bun with NODE_TLS_REJECT_UNAUTHORIZED=0)
//!
//! The `rigor trust` subcommand installs the CA into the macOS keychain.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rustls::ServerConfig;

/// Paths for the CA cert and key.
fn ca_cert_path() -> PathBuf {
    crate::paths::rigor_home().join("ca.pem")
}

fn ca_key_path() -> PathBuf {
    crate::paths::rigor_home().join("ca-key.pem")
}

/// The rigor CA — generates and caches per-host TLS certificates.
pub struct RigorCA {
    ca_key: rcgen::KeyPair,
    ca_cert_signed: rcgen::Certificate,
    /// Cache of per-host ServerConfigs to avoid regenerating certs.
    host_cache: Mutex<HashMap<String, Arc<ServerConfig>>>,
}

impl RigorCA {
    /// Load or generate the rigor CA certificate.
    /// Persists to ~/.rigor/ca.pem and ~/.rigor/ca-key.pem.
    pub fn load_or_generate() -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();

        let cert_path = ca_cert_path();
        let key_path = ca_key_path();

        // Ensure ~/.rigor/ exists
        if let Some(parent) = cert_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let (ca_key, ca_cert_signed) = if cert_path.exists() && key_path.exists() {
            // Load existing CA
            let key_pem = std::fs::read_to_string(&key_path).context("Failed to read CA key")?;
            let cert_pem = std::fs::read_to_string(&cert_path).context("Failed to read CA cert")?;

            let ca_key =
                rcgen::KeyPair::from_pem(&key_pem).context("Failed to parse CA key PEM")?;

            let ca_params = rcgen::CertificateParams::from_ca_cert_pem(&cert_pem)
                .context("Failed to parse CA cert PEM")?;

            let ca_cert_signed = ca_params
                .self_signed(&ca_key)
                .context("Failed to re-sign CA cert")?;

            crate::info_println!("rigor CA: loaded from {}", cert_path.display());
            (ca_key, ca_cert_signed)
        } else {
            // Generate new CA
            let mut ca_params = rcgen::CertificateParams::default();
            ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
            ca_params
                .distinguished_name
                .push(rcgen::DnType::CommonName, "rigor CA".to_string());
            ca_params
                .distinguished_name
                .push(rcgen::DnType::OrganizationName, "rigor".to_string());
            ca_params.key_usages = vec![
                rcgen::KeyUsagePurpose::KeyCertSign,
                rcgen::KeyUsagePurpose::CrlSign,
            ];

            let ca_key = rcgen::KeyPair::generate().context("Failed to generate CA key")?;

            let ca_cert_signed = ca_params
                .self_signed(&ca_key)
                .context("Failed to self-sign CA cert")?;

            // Persist
            std::fs::write(&cert_path, ca_cert_signed.pem())?;
            std::fs::write(&key_path, ca_key.serialize_pem())?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
            }

            crate::info_println!("rigor CA: generated new CA at {}", cert_path.display());
            eprintln!(
                "rigor CA: run `rigor trust` to install in macOS keychain for universal trust"
            );

            (ca_key, ca_cert_signed)
        };

        Ok(Self {
            ca_key,
            ca_cert_signed,
            host_cache: Mutex::new(HashMap::new()),
        })
    }

    /// Get or generate a TLS ServerConfig for the given hostname.
    /// The returned config presents a certificate for `hostname` signed by our CA.
    pub fn server_config_for_host(&self, hostname: &str) -> Result<Arc<ServerConfig>> {
        // Check cache first
        if let Ok(cache) = self.host_cache.lock() {
            if let Some(config) = cache.get(hostname) {
                return Ok(config.clone());
            }
        }

        // Generate a per-host cert signed by our CA
        let mut params = rcgen::CertificateParams::new(vec![hostname.to_string()])
            .context("Failed to create cert params")?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, hostname.to_string());
        params
            .distinguished_name
            .push(rcgen::DnType::OrganizationName, "rigor".to_string());

        let host_key = rcgen::KeyPair::generate().context("Failed to generate host key")?;

        let host_cert = params
            .signed_by(&host_key, &self.ca_cert_signed, &self.ca_key)
            .context("Failed to sign host cert with CA")?;

        let host_cert_der = host_cert.der().clone();
        let ca_cert_der = self.ca_cert_signed.der().clone();
        let host_key_der = host_key.serialize_der();

        // Build cert chain: host cert + CA cert
        let certs = vec![
            rustls::pki_types::CertificateDer::from(host_cert_der.to_vec()),
            rustls::pki_types::CertificateDer::from(ca_cert_der.to_vec()),
        ];
        let key = rustls::pki_types::PrivateKeyDer::try_from(host_key_der)
            .map_err(|e| anyhow::anyhow!("failed to parse host key: {}", e))?;

        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        let config = Arc::new(config);

        // Cache it
        if let Ok(mut cache) = self.host_cache.lock() {
            cache.insert(hostname.to_string(), config.clone());
        }

        Ok(config)
    }

    /// Path to the CA certificate (for `rigor trust`).
    pub fn ca_cert_path(&self) -> PathBuf {
        ca_cert_path()
    }
}

/// Install the rigor CA into the macOS login keychain.
/// After this, ALL apps on the system trust rigor's MITM certs.
pub fn install_ca_trust() -> Result<()> {
    let cert_path = ca_cert_path();
    if !cert_path.exists() {
        anyhow::bail!(
            "CA cert not found at {}. Run `rigor ground --mitm` first to generate it.",
            cert_path.display()
        );
    }

    eprintln!("rigor: installing CA cert into macOS login keychain...");
    eprintln!("rigor: you may be prompted for your password.");

    let output = std::process::Command::new("security")
        .args([
            "add-trusted-cert",
            "-d",
            "-r",
            "trustRoot",
            "-k",
            &format!(
                "{}/Library/Keychains/login.keychain-db",
                dirs::home_dir().unwrap_or_default().display() // rigor-home-ok
            ),
            &cert_path.to_string_lossy(),
        ])
        .output()?;

    if output.status.success() {
        eprintln!("rigor: CA cert installed successfully!");
        eprintln!("rigor: all apps will now trust rigor's MITM certificates.");
        eprintln!("rigor: to remove: rigor untrust");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to install CA cert: {}", stderr.trim())
    }
}

/// Remove the rigor CA from the macOS login keychain.
pub fn remove_ca_trust() -> Result<()> {
    let cert_path = ca_cert_path();
    if !cert_path.exists() {
        anyhow::bail!("CA cert not found at {}", cert_path.display());
    }

    let output = std::process::Command::new("security")
        .args(["remove-trusted-cert", "-d", &cert_path.to_string_lossy()])
        .output()?;

    if output.status.success() {
        eprintln!("rigor: CA cert removed from keychain.");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to remove CA cert: {}", stderr.trim())
    }
}

// === Legacy function for backward compatibility ===

/// Generate a self-signed TLS certificate for the given hostnames.
/// Used by the dedicated TLS listener (non-MITM mode).
pub fn generate_tls_config(hosts: &[&str]) -> Result<ServerConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut params =
        rcgen::CertificateParams::new(hosts.iter().map(|h| h.to_string()).collect::<Vec<_>>())?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, hosts[0].to_string());
    params
        .distinguished_name
        .push(rcgen::DnType::OrganizationName, "rigor".to_string());
    let key_pair = rcgen::KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;
    let cert_der = cert.der().clone();
    let key_der = key_pair.serialize_der();

    crate::info_println!("rigor daemon: generated self-signed cert for {:?}", hosts);

    let certs = vec![rustls::pki_types::CertificateDer::from(cert_der.to_vec())];
    let key = rustls::pki_types::PrivateKeyDer::try_from(key_der)
        .map_err(|e| anyhow::anyhow!("failed to parse private key: {}", e))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Helper: save RIGOR_HOME, set to tempdir/.rigor, run closure, restore.
    /// Uses the crate-wide RIGOR_HOME_TEST_LOCK to serialize across all
    /// test modules that mutate this env var.
    fn with_temp_rigor_home<F: FnOnce(&std::path::Path)>(f: F) {
        let _guard = crate::paths::RIGOR_HOME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let original = std::env::var("RIGOR_HOME").ok();
        let tmp = tempfile::TempDir::new().unwrap();
        let rigor_dir = tmp.path().join(".rigor");
        unsafe { std::env::set_var("RIGOR_HOME", &rigor_dir) };

        f(&rigor_dir);

        match original {
            Some(v) => unsafe { std::env::set_var("RIGOR_HOME", v) },
            None => unsafe { std::env::remove_var("RIGOR_HOME") },
        }
    }

    #[test]
    fn test_load_or_generate_creates_new_ca() {
        with_temp_rigor_home(|rigor_dir| {
            let ca = RigorCA::load_or_generate().expect("load_or_generate should succeed");
            let cert_path = rigor_dir.join("ca.pem");
            let key_path = rigor_dir.join("ca-key.pem");
            assert!(cert_path.exists(), "ca.pem should be created");
            assert!(key_path.exists(), "ca-key.pem should be created");
            // Verify we can get the CA cert path from the instance
            assert_eq!(ca.ca_cert_path(), cert_path);
        });
    }

    #[test]
    fn test_load_or_generate_roundtrip() {
        with_temp_rigor_home(|rigor_dir| {
            // First call: generates new CA
            let _ca1 = RigorCA::load_or_generate().expect("first load_or_generate should succeed");
            let cert_pem_1 = std::fs::read_to_string(rigor_dir.join("ca.pem")).unwrap();

            // Second call: loads existing CA from disk
            let _ca2 = RigorCA::load_or_generate().expect("second load_or_generate should succeed");
            let cert_pem_2 = std::fs::read_to_string(rigor_dir.join("ca.pem")).unwrap();

            assert_eq!(
                cert_pem_1, cert_pem_2,
                "CA cert PEM should be identical after roundtrip"
            );
        });
    }

    #[test]
    fn test_server_config_for_host_generates_valid_config() {
        with_temp_rigor_home(|_rigor_dir| {
            let ca = RigorCA::load_or_generate().unwrap();
            let config = ca.server_config_for_host("test.example.com");
            assert!(
                config.is_ok(),
                "server_config_for_host should return Ok for a valid hostname"
            );
        });
    }

    #[test]
    fn test_server_config_for_host_caches() {
        with_temp_rigor_home(|_rigor_dir| {
            let ca = RigorCA::load_or_generate().unwrap();

            let config1 = ca
                .server_config_for_host("cache-test.example.com")
                .expect("first call should succeed");
            let config2 = ca
                .server_config_for_host("cache-test.example.com")
                .expect("second call should succeed (from cache)");

            // Both should return valid configs; the cache hit is implicit
            // (second call succeeds without regenerating).
            // We can also verify they are the same Arc by pointer equality.
            assert!(
                Arc::ptr_eq(&config1, &config2),
                "cached call should return the same Arc"
            );
        });
    }

    #[test]
    fn test_install_ca_trust_fails_when_cert_missing() {
        with_temp_rigor_home(|rigor_dir| {
            // Create the .rigor directory but do NOT generate any CA
            std::fs::create_dir_all(rigor_dir).unwrap();
            let result = install_ca_trust();
            assert!(
                result.is_err(),
                "install_ca_trust should fail when ca.pem is missing"
            );
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("CA cert not found"),
                "error should mention 'CA cert not found', got: {}",
                err_msg
            );
        });
    }

    #[test]
    fn test_generate_tls_config_creates_self_signed() {
        // Pure function -- does not use rigor_home(), no ENV_LOCK needed
        let result = generate_tls_config(&["localhost", "127.0.0.1"]);
        assert!(
            result.is_ok(),
            "generate_tls_config should produce a valid self-signed config"
        );
    }
}
