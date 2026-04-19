//! Pluggable claim evaluator pipeline.
//!
//! The original claim evaluation flow in `daemon/proxy.rs` and `lib.rs` is a
//! monolithic pipeline: extract → Rego eval → (optional LLM judge). This
//! module abstracts the "evaluate one claim against one constraint" step
//! behind the [`ClaimEvaluator`] trait so that different evaluator "agents"
//! (regex/Rego, semantic LLM, future specialists) can be registered and
//! routed to based on what each one [`can_evaluate`].
//!
//! See [`pipeline`] for the trait, [`EvalResult`], and [`EvaluatorPipeline`].

pub mod pipeline;

pub use pipeline::{
    ClaimEvaluator, EvalResult, EvaluatorPipeline, RegexEvaluator, SemanticEvaluator,
};
