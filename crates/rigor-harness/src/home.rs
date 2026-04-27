use std::path::PathBuf;

/// An isolated HOME directory backed by a TempDir.
///
/// Creates a temporary directory with a `.rigor/` subdirectory, suitable for
/// use as `HOME` in subprocess tests via `Command::env("HOME", home.home_str())`.
///
/// Does NOT call `std::env::set_var("HOME", ...)` -- that would be unsafe in
/// parallel tests. Callers pass the path explicitly to `Command::env()`.
pub struct IsolatedHome {
    _temp: tempfile::TempDir,
    /// The root path (acts as HOME).
    pub path: PathBuf,
    /// The `.rigor/` subdirectory inside the isolated home.
    pub rigor_dir: PathBuf,
}

impl IsolatedHome {
    pub fn new() -> Self {
        let temp = tempfile::TempDir::new().expect("failed to create temp HOME");
        let path = temp.path().to_path_buf();
        let rigor_dir = path.join(".rigor");
        std::fs::create_dir_all(&rigor_dir).expect("failed to create .rigor dir");
        Self {
            _temp: temp,
            path,
            rigor_dir,
        }
    }

    /// Write a rigor.yaml into the isolated home directory.
    pub fn write_rigor_yaml(&self, content: &str) -> PathBuf {
        let yaml_path = self.path.join("rigor.yaml");
        std::fs::write(&yaml_path, content).expect("write rigor.yaml");
        yaml_path
    }

    /// Get HOME value suitable for `Command::env("HOME", ...)`.
    pub fn home_str(&self) -> String {
        self.path.to_string_lossy().to_string()
    }

    /// Get the `.rigor` directory path (for CA certs, PID files, etc.).
    pub fn rigor_dir_str(&self) -> String {
        self.rigor_dir.to_string_lossy().to_string()
    }
}

impl Default for IsolatedHome {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_isolated_home_creates_rigor_dir() {
        let home = IsolatedHome::new();
        assert!(home.rigor_dir.exists(), ".rigor/ must exist");
        assert!(home.rigor_dir.is_dir(), ".rigor/ must be a directory");
    }

    #[test]
    fn test_write_rigor_yaml() {
        let home = IsolatedHome::new();
        let yaml = "constraints:\n  - id: test\n";
        let path = home.write_rigor_yaml(yaml);
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, yaml);
    }

    #[test]
    fn test_home_str_returns_valid_path() {
        let home = IsolatedHome::new();
        let s = home.home_str();
        let p = PathBuf::from(&s);
        assert!(p.is_absolute(), "home_str must return an absolute path");
        assert!(p.exists(), "home_str path must exist");
    }
}
