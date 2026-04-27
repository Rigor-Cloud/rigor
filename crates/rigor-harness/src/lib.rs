//! Test harness primitives for rigor.
//!
//! Provides IsolatedHome, TestCA, MockLlmServer, and SSE helpers
//! for integration testing without touching real `~/.rigor/` or
//! requiring live LLM API credentials.

pub mod ca;
pub mod env_lock;
pub mod home;
pub mod mock_llm;
pub mod proxy;
pub mod sse;
pub mod subprocess;

pub use ca::TestCA;
pub use home::IsolatedHome;
pub use mock_llm::{MockLlmServer, MockLlmServerBuilder, ReceivedRequest};
pub use proxy::TestProxy;
pub use sse::{extract_text_from_sse, parse_sse_events, SseFormat};
pub use subprocess::{
    default_hook_input, extract_decision, parse_response, run_rigor, run_rigor_with_claims,
    run_rigor_with_env,
};
