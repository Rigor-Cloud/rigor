use std::env;
use std::path::{Path, PathBuf};

/// Find rigor.lock by walking up the directory tree from cwd.
/// Returns None if not found (no config = always allow).
pub fn find_rigor_lock() -> Option<PathBuf> {
    find_rigor_lock_from(env::current_dir().ok()?)
}

/// Find rigor.lock starting from a specific directory.
/// Useful for testing with controlled paths.
pub fn find_rigor_lock_from(start: impl AsRef<Path>) -> Option<PathBuf> {
    find_file_from(start, "rigor.lock")
}

/// Find rigor.yaml by walking up the directory tree from cwd.
/// Returns None if not found (no config = always allow).
pub fn find_rigor_yaml() -> Option<PathBuf> {
    find_rigor_yaml_from(env::current_dir().ok()?)
}

/// Find rigor.yaml starting from a specific directory.
/// Useful for testing with controlled paths.
pub fn find_rigor_yaml_from(start: impl AsRef<Path>) -> Option<PathBuf> {
    find_file_from(start, "rigor.yaml")
}

/// Generic file finder: walk up from `start` looking for `filename`.
fn find_file_from(start: impl AsRef<Path>, filename: &str) -> Option<PathBuf> {
    let mut current = start.as_ref().to_path_buf();

    loop {
        let candidate = current.join(filename);
        if candidate.exists() {
            return Some(candidate);
        }

        // Move to parent directory
        if !current.pop() {
            break; // Reached filesystem root
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_find_rigor_lock_in_current_dir() {
        let temp = TempDir::new().unwrap();
        let lock_path = temp.path().join("rigor.lock");
        fs::write(&lock_path, "# test config").unwrap();

        let found = find_rigor_lock_from(temp.path());
        assert_eq!(found, Some(lock_path));
    }

    #[test]
    fn test_find_rigor_lock_in_parent() {
        let temp = TempDir::new().unwrap();
        let lock_path = temp.path().join("rigor.lock");
        fs::write(&lock_path, "# test config").unwrap();

        let subdir = temp.path().join("subdir");
        fs::create_dir(&subdir).unwrap();

        let found = find_rigor_lock_from(&subdir);
        assert_eq!(found, Some(lock_path));
    }

    #[test]
    fn test_find_rigor_lock_not_found() {
        let temp = TempDir::new().unwrap();
        // No rigor.lock created

        let found = find_rigor_lock_from(temp.path());
        assert_eq!(found, None);
    }
}
