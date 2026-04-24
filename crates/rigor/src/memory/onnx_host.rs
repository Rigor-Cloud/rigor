//! ONNX model host — feature-gated shared infrastructure for local ML inference.
//!
//! Provides [`OnnxModelHost`] for downloading, caching, and loading ONNX models
//! from HuggingFace Hub. Used by Kompress (Phase 1D) and the safety discriminator
//! (Phase 4F).
//!
//! Gated behind the `onnx` Cargo feature. Enable with:
//! ```sh
//! cargo build -p rigor --features onnx
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use tracing;

// ── Trait abstraction (REQ-009) ─────────────────────────────────────────────

/// Backend-agnostic inference host trait.
///
/// Separates model loading from the concrete ONNX runtime so that Kompress
/// and ModernBERT depend on the trait, not the runtime. Future backends
/// (e.g., candle, burn) can implement this without touching consumers.
pub trait InferenceHost: Send + Sync {
    /// Load (or retrieve from cache) a model identified by `model_id`.
    ///
    /// `model_id` is typically a HuggingFace repo path like
    /// `"chopratejas/kompress-base"`. `filename` is the ONNX file within
    /// the repo (e.g., `"model.onnx"`). `expected_sha256` is the hex-encoded
    /// SHA-256 digest of the file for integrity verification.
    ///
    /// Returns the filesystem path to the cached model file.
    fn load(
        &self,
        model_id: &str,
        filename: &str,
        expected_sha256: &str,
    ) -> Result<PathBuf>;
}

// ── ONNX implementation ────────────────────────────────────────────────────

/// Local ONNX model host with HuggingFace Hub download + SHA-256 verified cache.
///
/// Cache layout: `<cache_dir>/<sha256_hex>/model.onnx`
///
/// The cache key is the expected SHA-256 digest, making it content-addressed.
/// If a cached file exists and matches the digest, no download occurs.
pub struct OnnxModelHost {
    cache_dir: PathBuf,
}

impl OnnxModelHost {
    /// Create a new host with the default cache directory at `rigor_home()/models`.
    pub fn new() -> Self {
        let cache_dir = crate::paths::rigor_home().join("models");
        Self { cache_dir }
    }

    /// Create a new host with a custom cache directory (useful for testing).
    pub fn with_cache_dir(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// Return the cache directory path.
    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    /// Compute SHA-256 hex digest of a file on disk.
    fn verify_sha256(path: &PathBuf, expected: &str) -> Result<()> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read model file for hash verification: {}", path.display()))?;
        let digest = Sha256::digest(&bytes);
        let actual = format!("{:x}", digest);
        if actual != expected {
            anyhow::bail!(
                "SHA-256 mismatch for {}: expected {}, got {}",
                path.display(),
                expected,
                actual,
            );
        }
        Ok(())
    }

    /// Download a file from HuggingFace Hub into the cache directory.
    ///
    /// Uses `hf_hub::api::sync::Api` for blocking download. The file is placed
    /// at `<cache_dir>/<sha256>/<filename>`.
    fn download_from_hf(
        &self,
        model_id: &str,
        filename: &str,
        expected_sha256: &str,
    ) -> Result<PathBuf> {
        let target_dir = self.cache_dir.join(expected_sha256);
        std::fs::create_dir_all(&target_dir)
            .with_context(|| format!("failed to create model cache dir: {}", target_dir.display()))?;

        let target_path = target_dir.join(filename);

        tracing::info!(
            model_id = model_id,
            filename = filename,
            cache_path = %target_path.display(),
            "downloading model from HuggingFace Hub",
        );

        let api = hf_hub::api::sync::Api::new()
            .context("failed to initialize HuggingFace Hub API")?;

        let repo = api.model(model_id.to_string());
        let downloaded_path = repo
            .get(filename)
            .with_context(|| format!("failed to download {}/{}", model_id, filename))?;

        // hf-hub downloads to its own cache; copy to our content-addressed cache.
        std::fs::copy(&downloaded_path, &target_path).with_context(|| {
            format!(
                "failed to copy downloaded model from {} to {}",
                downloaded_path.display(),
                target_path.display(),
            )
        })?;

        Ok(target_path)
    }
}

impl InferenceHost for OnnxModelHost {
    fn load(
        &self,
        model_id: &str,
        filename: &str,
        expected_sha256: &str,
    ) -> Result<PathBuf> {
        let cached_path = self.cache_dir.join(expected_sha256).join(filename);

        // Check cache first.
        if cached_path.exists() {
            tracing::debug!(
                path = %cached_path.display(),
                "model found in cache, verifying integrity",
            );
            match Self::verify_sha256(&cached_path, expected_sha256) {
                Ok(()) => {
                    tracing::info!(
                        model_id = model_id,
                        path = %cached_path.display(),
                        "using cached model (SHA-256 verified)",
                    );
                    return Ok(cached_path);
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "cached model failed integrity check, re-downloading",
                    );
                    // Remove corrupt file before re-download.
                    let _ = std::fs::remove_file(&cached_path);
                }
            }
        }

        // Download from HuggingFace Hub.
        let path = self.download_from_hf(model_id, filename, expected_sha256)?;

        // Verify downloaded file integrity.
        Self::verify_sha256(&path, expected_sha256)
            .context("downloaded model failed SHA-256 verification")?;

        tracing::info!(
            model_id = model_id,
            path = %path.display(),
            "model downloaded and verified",
        );

        Ok(path)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn new_uses_rigor_home_models() {
        let _guard = crate::paths::RIGOR_HOME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("RIGOR_HOME", tmp.path()) };

        let host = OnnxModelHost::new();
        assert_eq!(host.cache_dir(), &tmp.path().join("models"));

        unsafe { std::env::remove_var("RIGOR_HOME") };
    }

    #[test]
    fn with_cache_dir_uses_custom_path() {
        let custom = PathBuf::from("/tmp/custom-models");
        let host = OnnxModelHost::with_cache_dir(custom.clone());
        assert_eq!(host.cache_dir(), &custom);
    }

    #[test]
    fn verify_sha256_valid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.bin");
        let content = b"hello rigor onnx host";
        fs::write(&file_path, content).unwrap();

        // Compute expected hash.
        let expected = format!("{:x}", Sha256::digest(content));
        assert!(OnnxModelHost::verify_sha256(&file_path, &expected).is_ok());
    }

    #[test]
    fn verify_sha256_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test.bin");
        fs::write(&file_path, b"real content").unwrap();

        let result = OnnxModelHost::verify_sha256(&file_path, "0000000000000000");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("SHA-256 mismatch"), "unexpected error: {}", msg);
    }

    #[test]
    fn verify_sha256_missing_file() {
        let path = PathBuf::from("/tmp/nonexistent-rigor-test-file.bin");
        let result = OnnxModelHost::verify_sha256(&path, "deadbeef");
        assert!(result.is_err());
    }

    #[test]
    fn load_returns_cached_file_when_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let host = OnnxModelHost::with_cache_dir(tmp.path().to_path_buf());

        // Pre-populate cache.
        let content = b"fake onnx model bytes";
        let sha = format!("{:x}", Sha256::digest(content));
        let model_dir = tmp.path().join(&sha);
        fs::create_dir_all(&model_dir).unwrap();
        let model_file = model_dir.join("model.onnx");
        fs::write(&model_file, content).unwrap();

        // load() should return cached path without attempting download.
        let result = host.load("fake/model", "model.onnx", &sha);
        assert!(result.is_ok(), "load failed: {:?}", result.err());
        assert_eq!(result.unwrap(), model_file);
    }

    #[test]
    fn load_rejects_corrupt_cache_and_errors_on_download() {
        let tmp = tempfile::tempdir().unwrap();
        let host = OnnxModelHost::with_cache_dir(tmp.path().to_path_buf());

        // Pre-populate cache with WRONG content.
        let sha = format!("{:x}", Sha256::digest(b"expected content"));
        let model_dir = tmp.path().join(&sha);
        fs::create_dir_all(&model_dir).unwrap();
        let model_file = model_dir.join("model.onnx");
        fs::write(&model_file, b"corrupt content").unwrap();

        // load() should detect corruption and try to re-download.
        // Since we're using a fake model_id, the download will fail.
        let result = host.load("nonexistent/model", "model.onnx", &sha);
        assert!(result.is_err(), "expected error for corrupt cache + failed download");

        // The corrupt file should have been removed.
        assert!(
            !model_file.exists(),
            "corrupt cached file should have been deleted",
        );
    }

    #[test]
    fn inference_host_trait_is_object_safe() {
        // Verify InferenceHost can be used as a trait object (dyn dispatch).
        fn _assert_object_safe(_: &dyn InferenceHost) {}
    }

    #[test]
    fn cache_dir_created_on_download_attempt() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("nested").join("models");
        let host = OnnxModelHost::with_cache_dir(cache_dir.clone());

        // Attempt download (will fail, but should create cache dir structure).
        let sha = "a".repeat(64);
        let _ = host.load("fake/repo", "model.onnx", &sha);

        // The sha-named directory should have been created.
        assert!(
            cache_dir.join(&sha).exists(),
            "cache subdirectory should be created even if download fails",
        );
    }
}
