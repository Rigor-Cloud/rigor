//! Test harness primitives for rigor.
//!
//! Provides IsolatedHome, TestCA, MockLlmServer, and SSE helpers
//! for integration testing without touching real `~/.rigor/` or
//! requiring live LLM API credentials.

pub mod home;
pub mod ca;
pub mod mock_llm;
pub mod sse;

pub use home::IsolatedHome;
pub use ca::TestCA;
pub use mock_llm::{MockLlmServer, MockLlmServerBuilder};
pub use sse::{SseFormat, parse_sse_events, extract_text_from_sse};
