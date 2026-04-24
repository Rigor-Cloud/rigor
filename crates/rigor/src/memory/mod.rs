//! Cross-session memory for rigor.
//!
//! Two complementary stores:
//!
//! - **Episodic memory**: summaries of past sessions (violations, outcomes). Used
//!   to remind a fresh AI process of what went wrong before.
//! - **Semantic memory**: learned patterns about this codebase — which file paths
//!   are associated with which violations, which models tend to produce which
//!   types of false claims.
//!
//! Both are persisted in `~/.rigor/memory.json`.

pub mod content_store;
pub mod episodic;
#[cfg(feature = "onnx")]
pub mod onnx_host;

pub use content_store::{
    hash_bytes, hash_hex, Category, ContentStoreBackend, Hash, InMemoryBackend, SearchResult,
    StoredContent,
};
pub use episodic::{
    EpisodicMemory, MemoryStore, ModelPattern, PathPattern, SemanticMemory, SessionEpisode,
};
