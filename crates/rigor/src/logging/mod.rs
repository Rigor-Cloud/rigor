pub mod annotate;
pub mod backend;
pub mod query;
pub mod session;
pub mod session_registry;
pub mod types;
pub mod violation_log;

// Re-export key types
pub use backend::{LogQuery, ViolationLogBackend};
pub use types::{ClaimSource, SessionMetadata, ViolationLogEntry};
pub use violation_log::ViolationLogger;
