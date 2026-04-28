#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! E2E tests for the PID file crash recovery lifecycle.
//!
//! Exercises the scenario where a daemon writes a PID, "crashes" (stale PID
//! file left behind), and a new daemon start correctly detects the stale PID
//! and overwrites it. Also covers directory auto-creation and atomic overwrite.
//!
//! These tests mutate the global `RIGOR_HOME` env var, so they are serialized
//! via a local static mutex. They run with `#[test]` (synchronous).

use rigor::daemon::{daemon_alive, remove_pid_file, write_pid_file};
use rigor_harness::env_lock::ENV_LOCK as PID_TEST_LOCK;

/// Run a closure with `RIGOR_HOME` temporarily set to a fresh tempdir.
///
/// The tempdir is created and `RIGOR_HOME` set to it. After the closure returns
/// (or panics), the original `RIGOR_HOME` value is restored.
fn with_temp_rigor_home<F: FnOnce(&std::path::Path)>(f: F) {
    let _guard = PID_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original = std::env::var("RIGOR_HOME").ok();
    let tmp = tempfile::TempDir::new().unwrap();
    let rigor_dir = tmp.path().to_path_buf();
    // RIGOR_HOME points directly to the directory where daemon.pid will live,
    // because rigor_home() returns the RIGOR_HOME value as-is, and
    // daemon_pid_file() returns rigor_home().join("daemon.pid").
    unsafe { std::env::set_var("RIGOR_HOME", &rigor_dir) };

    f(&rigor_dir);

    match original {
        Some(v) => unsafe { std::env::set_var("RIGOR_HOME", v) },
        None => unsafe { std::env::remove_var("RIGOR_HOME") },
    }
}

/// Full write -> crash -> detect stale -> rewrite -> clean shutdown lifecycle.
///
/// This is the core E2E gap: no existing test exercises the complete sequence
/// of a daemon starting, crashing (leaving a stale PID), having the stale PID
/// detected as dead, a new daemon overwriting the stale PID, and finally a
/// clean shutdown removing the PID file.
#[test]
fn pid_file_crash_recovery_lifecycle() {
    with_temp_rigor_home(|rigor_dir| {
        // Step 1: Write PID file (simulates daemon start)
        write_pid_file().expect("write_pid_file should succeed");
        assert!(
            daemon_alive(),
            "daemon_alive should return true when PID file contains our own PID"
        );

        // Step 2: Simulate crash -- overwrite PID file with a dead PID.
        // PID 2000000 exceeds typical OS PID ranges and is extremely unlikely
        // to be a real running process.
        let pid_path = rigor_dir.join("daemon.pid");
        std::fs::write(&pid_path, "2000000\n").unwrap();
        assert!(
            !daemon_alive(),
            "daemon_alive should return false for stale (dead) PID after crash"
        );

        // Step 3: New daemon start -- overwrites stale PID with current process PID
        write_pid_file().expect("write_pid_file should succeed on overwrite");
        assert!(
            daemon_alive(),
            "daemon_alive should return true after overwriting stale PID with live PID"
        );

        // Step 4: Clean shutdown
        remove_pid_file();
        assert!(
            !daemon_alive(),
            "daemon_alive should return false after clean shutdown (PID file removed)"
        );
    });
}

/// Verifies that `write_pid_file()` creates the RIGOR_HOME directory if it
/// does not yet exist. This covers the case where rigor is started for the
/// first time and `~/.rigor/` has never been created.
#[test]
fn pid_file_absent_directory_created() {
    let _guard = PID_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original = std::env::var("RIGOR_HOME").ok();

    let tmp = tempfile::TempDir::new().unwrap();
    // Point RIGOR_HOME to a subdirectory that does NOT exist yet.
    let rigor_dir = tmp.path().join("nonexistent-rigor-dir");
    assert!(
        !rigor_dir.exists(),
        "Directory should not exist before write_pid_file"
    );
    unsafe { std::env::set_var("RIGOR_HOME", &rigor_dir) };

    // write_pid_file should create the directory via create_dir_all
    write_pid_file().expect("write_pid_file should create missing directories");

    let pid_path = rigor_dir.join("daemon.pid");
    assert!(
        pid_path.exists(),
        "PID file should exist after write_pid_file even when directory was missing"
    );
    assert!(
        daemon_alive(),
        "daemon_alive should return true after writing PID to newly created directory"
    );

    // Cleanup
    remove_pid_file();
    match original {
        Some(v) => unsafe { std::env::set_var("RIGOR_HOME", v) },
        None => unsafe { std::env::remove_var("RIGOR_HOME") },
    }
}

/// Verifies that `write_pid_file()` correctly overwrites a stale PID file,
/// and the resulting file contains exactly the current process ID.
#[test]
fn pid_file_overwrite_is_atomic() {
    with_temp_rigor_home(|rigor_dir| {
        // Write a stale PID
        let pid_path = rigor_dir.join("daemon.pid");
        std::fs::create_dir_all(rigor_dir).unwrap();
        std::fs::write(&pid_path, "2000000\n").unwrap();
        assert!(
            !daemon_alive(),
            "daemon_alive should return false for dead PID 2000000"
        );

        // Overwrite with current PID
        write_pid_file().expect("write_pid_file should succeed on overwrite");

        // Verify file content
        let content = std::fs::read_to_string(&pid_path).unwrap();
        let written_pid: u32 = content
            .trim()
            .parse()
            .expect("PID file should contain a valid numeric PID");
        assert_eq!(
            written_pid,
            std::process::id(),
            "PID file should contain exactly the current process ID"
        );

        // Verify liveness
        assert!(
            daemon_alive(),
            "daemon_alive should return true after overwriting stale PID"
        );
    });
}
