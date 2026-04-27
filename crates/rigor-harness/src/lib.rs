//! Test harness primitives for rigor.
//!
//! Provides IsolatedHome, TestCA, MockLlmServer, and SSE helpers
//! for integration testing without touching real `~/.rigor/` or
//! requiring live LLM API credentials.

pub mod home;
pub mod ca;
pub mod env_lock;
pub mod mock_llm;
pub mod proxy;
pub mod sse;
pub mod subprocess;

pub use home::IsolatedHome;
pub use ca::TestCA;
pub use mock_llm::{MockLlmServer, MockLlmServerBuilder, ReceivedRequest};
pub use proxy::TestProxy;
pub use sse::{SseFormat, parse_sse_events, extract_text_from_sse};
pub use subprocess::{run_rigor, run_rigor_with_claims, run_rigor_with_env, parse_response, extract_decision, default_hook_input};
