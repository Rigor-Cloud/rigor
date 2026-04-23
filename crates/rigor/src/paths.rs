use std::path::PathBuf;

/// Returns the rigor data directory.
///
/// Resolution order:
/// 1. `RIGOR_HOME` environment variable (if set and non-empty)
/// 2. `$HOME/.rigor/` via `dirs::home_dir()`
///
/// Panics if neither is available. In practice, HOME is always set on
/// macOS and Linux; RIGOR_HOME is set by test fixtures.
pub fn rigor_home() -> PathBuf {
    if let Ok(val) = std::env::var("RIGOR_HOME") {
        if !val.is_empty() {
            return PathBuf::from(val);
        }
    }
    dirs::home_dir()
        .expect("Cannot determine home directory (set RIGOR_HOME or HOME)")
        .join(".rigor")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Global mutex to serialize env-var-mutating tests.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn rigor_home_returns_rigor_home_env_when_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("RIGOR_HOME").ok();
        unsafe { std::env::set_var("RIGOR_HOME", "/tmp/test-rigor-home") };
        let result = rigor_home();
        // Restore
        match original {
            Some(v) => unsafe { std::env::set_var("RIGOR_HOME", v) },
            None => unsafe { std::env::remove_var("RIGOR_HOME") },
        }
        assert_eq!(result, PathBuf::from("/tmp/test-rigor-home"));
    }

    #[test]
    fn rigor_home_ignores_empty_rigor_home_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("RIGOR_HOME").ok();
        unsafe { std::env::set_var("RIGOR_HOME", "") };
        let result = rigor_home();
        // Restore
        match original {
            Some(v) => unsafe { std::env::set_var("RIGOR_HOME", v) },
            None => unsafe { std::env::remove_var("RIGOR_HOME") },
        }
        // Should fall back to dirs::home_dir()/.rigor
        let expected = dirs::home_dir().unwrap().join(".rigor");
        assert_eq!(result, expected);
    }

    #[test]
    fn rigor_home_falls_back_when_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("RIGOR_HOME").ok();
        unsafe { std::env::remove_var("RIGOR_HOME") };
        let result = rigor_home();
        // Restore
        match original {
            Some(v) => unsafe { std::env::set_var("RIGOR_HOME", v) },
            None => {} // already removed
        }
        let expected = dirs::home_dir().unwrap().join(".rigor");
        assert_eq!(result, expected);
    }

    #[test]
    fn rigor_home_fallback_ends_in_dot_rigor() {
        let _guard = ENV_LOCK.lock().unwrap();
        let original = std::env::var("RIGOR_HOME").ok();
        unsafe { std::env::remove_var("RIGOR_HOME") };
        let result = rigor_home();
        // Restore
        match original {
            Some(v) => unsafe { std::env::set_var("RIGOR_HOME", v) },
            None => {}
        }
        assert!(
            result.ends_with(".rigor"),
            "Expected path to end with .rigor, got: {:?}",
            result
        );
    }
}
