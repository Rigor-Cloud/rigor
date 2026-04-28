//! Process-wide test environment lock.
//!
//! All test binaries that mutate process-global env vars (`RIGOR_HOME`,
//! `RIGOR_TARGET_API`, `RIGOR_NO_RETRY`, …) must serialize through this single
//! `Mutex<()>`.
//!
//! Why one shared static: `cargo test` runs each integration-test file as its
//! own binary, but they all link against `rigor-harness` as the same
//! compilation unit. A single `pub static` in this crate is therefore the
//! only `Mutex` instance every test binary sees, so locking it interlocks all
//! env-var-mutating tests across the whole workspace.
//!
//! Background: brutal-code-critic 2026-04-27 (C6) found four separate
//! `static ENV_LOCK: Mutex<()>` declarations in different test files. Each
//! statics was its own instance, so parallel tests in different binaries did
//! not actually serialize and could race on `std::env::set_var`.

/// Acquire-this-then-mutate-env. Held for the lifetime of the returned guard.
///
/// Use `let _guard = ENV_LOCK.lock().unwrap();` (or
/// `.unwrap_or_else(|e| e.into_inner())` if you want to ignore poisoning from
/// previous panicking tests).
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
