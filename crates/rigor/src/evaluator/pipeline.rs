//! Pluggable claim evaluator pipeline.
//!
//! Defines the [`ClaimEvaluator`] trait, the [`EvalResult`] return type, and
//! the [`EvaluatorPipeline`] which routes a claim/constraint pair to the
//! first registered evaluator that [`ClaimEvaluator::can_evaluate`] it.
//!
//! Two built-in evaluators ship with this module:
//!
//! - [`RegexEvaluator`] — the default Rego/regex path. Wraps a
//!   [`crate::policy::PolicyEngine`] and evaluates a single claim against a
//!   single constraint by building a one-claim `EvaluationInput`.
//! - [`SemanticEvaluator`] — marker evaluator for constraints that are
//!   better handled by the async LLM-as-judge path in `daemon/proxy.rs`
//!   (see `score_claim_relevance`). Returns a deferred, non-violating
//!   result synchronously; the real semantic scoring continues to run
//!   out-of-band.
//!
//! The routing contract is intentionally simple: the pipeline asks each
//! evaluator in registration order whether it [`can_evaluate`] the pair, and
//! uses the first match. If nothing matches, the pipeline falls back to the
//! default Rego evaluator so existing behaviour is preserved.

use crate::claim::Claim;
use crate::constraint::{Constraint, RigorConfig};
use crate::policy::{EvaluationInput, PolicyEngine};

/// Result of evaluating a single claim against a single constraint.
#[derive(Debug, Clone)]
pub struct EvalResult {
    pub violated: bool,
    pub confidence: f64,
    pub reason: String,
}

impl EvalResult {
    pub fn allow(reason: impl Into<String>) -> Self {
        Self {
            violated: false,
            confidence: 1.0,
            reason: reason.into(),
        }
    }

    pub fn violation(reason: impl Into<String>, confidence: f64) -> Self {
        Self {
            violated: true,
            confidence,
            reason: reason.into(),
        }
    }
}

/// Pluggable evaluator for a single (claim, constraint) pair.
///
/// Implementors should make [`name`] a stable short identifier and
/// [`can_evaluate`] cheap — the pipeline calls it on every candidate pairing
/// until it finds a match.
///
/// [`name`]: ClaimEvaluator::name
/// [`can_evaluate`]: ClaimEvaluator::can_evaluate
pub trait ClaimEvaluator: Send + Sync {
    /// Short stable name (e.g. `"regex"`, `"semantic"`). Used for logs and
    /// metrics.
    fn name(&self) -> &str;

    /// Return `true` if this evaluator should handle the given pair. The
    /// first registered evaluator that returns `true` wins.
    fn can_evaluate(&self, claim: &Claim, constraint: &Constraint) -> bool;

    /// Evaluate the pair. Must not panic. On internal error, prefer
    /// returning an `allow` result (fail-open) with a descriptive `reason`.
    fn evaluate(&self, claim: &Claim, constraint: &Constraint) -> EvalResult;
}

/// The default Rego-backed evaluator. Claims whose constraints have a
/// non-empty `rego` snippet are handled here. Internally wraps a
/// [`PolicyEngine`] and evaluates a single-claim input.
pub struct RegexEvaluator {
    /// Full config used to construct a per-call engine. Cloning a
    /// `PolicyEngine` is O(constraints) — cheap for the small constraint
    /// counts rigor targets — so we lazily clone and mutate.
    engine: PolicyEngine,
}

impl RegexEvaluator {
    /// Construct from a [`RigorConfig`]. Returns an error only if loading
    /// the engine itself fails; individual invalid constraints are already
    /// skipped by [`PolicyEngine::new`] (fail-open).
    pub fn new(config: &RigorConfig) -> anyhow::Result<Self> {
        Ok(Self {
            engine: PolicyEngine::new(config)?,
        })
    }
}

impl ClaimEvaluator for RegexEvaluator {
    fn name(&self) -> &str {
        "regex"
    }

    /// The regex/Rego evaluator is the catch-all. It can evaluate any
    /// constraint that has a non-empty Rego snippet — which is every
    /// constraint authored via `rigor init`, `rigor:map`, or manual edit.
    fn can_evaluate(&self, _claim: &Claim, constraint: &Constraint) -> bool {
        !constraint.rego.trim().is_empty()
    }

    fn evaluate(&self, claim: &Claim, constraint: &Constraint) -> EvalResult {
        // Build a one-claim input so we can reuse the policy engine's
        // constraint-specific rule path.
        let input = EvaluationInput {
            claims: vec![claim.clone()],
        };

        // Clone the engine so `&self` evaluation doesn't require interior
        // mutability. `PolicyEngine: Clone` and its cost is linear in
        // loaded constraints — acceptable for the pipeline's single-pair
        // evaluations.
        let mut engine = self.engine.clone();
        match engine.evaluate(&input) {
            Ok(raw) => {
                // Only surface violations for the constraint we're asking
                // about — the engine evaluates all loaded rules in one pass.
                if let Some(v) = raw.iter().find(|v| v.constraint_id == constraint.id && v.violated)
                {
                    EvalResult::violation(v.reason.clone(), claim.confidence)
                } else {
                    EvalResult::allow(format!("no violation of {}", constraint.id))
                }
            }
            Err(e) => {
                // Fail-open: never block on evaluator errors.
                EvalResult::allow(format!("regex evaluator error (fail-open): {}", e))
            }
        }
    }
}

/// Semantic (LLM-as-judge) evaluator.
///
/// The real semantic scoring runs asynchronously in
/// `daemon/proxy.rs::score_claim_relevance` — it calls the judge model,
/// caches results, and broadcasts them to the dashboard. This synchronous
/// evaluator is a *routing marker*: it declares ownership of constraints
/// tagged `semantic` (or explicitly opted in via the `rigor` field being
/// empty, meaning "no rule → must be judged"), and returns a non-violating
/// deferred result so the pipeline doesn't double-evaluate via Rego.
///
/// Downstream, the async judge path continues to run on its own schedule
/// and emits `ClaimRelevance` events. Making this evaluator a first-class
/// member of the pipeline means future versions can synchronously block on
/// the judge if desired without changing the call sites.
pub struct SemanticEvaluator {
    /// Case-insensitive tag that marks a constraint as semantic.
    tag: String,
}

impl SemanticEvaluator {
    pub fn new() -> Self {
        Self {
            tag: "semantic".to_string(),
        }
    }

    pub fn with_tag(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }
}

impl Default for SemanticEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaimEvaluator for SemanticEvaluator {
    fn name(&self) -> &str {
        "semantic"
    }

    fn can_evaluate(&self, _claim: &Claim, constraint: &Constraint) -> bool {
        // A constraint routes here when either:
        //   1. It's explicitly tagged `semantic`, or
        //   2. It has no Rego snippet at all (nothing for regex to do).
        let tagged = constraint
            .tags
            .iter()
            .any(|t| t.eq_ignore_ascii_case(&self.tag));
        tagged || constraint.rego.trim().is_empty()
    }

    fn evaluate(&self, _claim: &Claim, constraint: &Constraint) -> EvalResult {
        // Defer: the real scoring happens asynchronously in the proxy.
        EvalResult::allow(format!(
            "semantic constraint {} deferred to LLM-as-judge",
            constraint.id
        ))
    }
}

/// Registry + router. Holds an ordered list of evaluators; the first one
/// that [`ClaimEvaluator::can_evaluate`] a (claim, constraint) pair wins.
///
/// If no evaluator matches, the pipeline falls back to the embedded default
/// Rego evaluator (constructed from the [`RigorConfig`] passed to
/// [`EvaluatorPipeline::new`]) so existing behaviour is preserved.
pub struct EvaluatorPipeline {
    evaluators: Vec<Box<dyn ClaimEvaluator>>,
    /// Fallback regex evaluator used when no registered evaluator matches.
    fallback: Option<RegexEvaluator>,
}

impl EvaluatorPipeline {
    /// Empty pipeline with no fallback. Use [`with_default_fallback`] in
    /// production paths so unmatched constraints still get Rego evaluation.
    ///
    /// [`with_default_fallback`]: Self::with_default_fallback
    pub fn new() -> Self {
        Self {
            evaluators: Vec::new(),
            fallback: None,
        }
    }

    /// Pipeline seeded with a default Rego fallback built from `config`.
    /// Equivalent to `new()` followed by attaching a [`RegexEvaluator`] as
    /// the fallback.
    pub fn with_default_fallback(config: &RigorConfig) -> anyhow::Result<Self> {
        Ok(Self {
            evaluators: Vec::new(),
            fallback: Some(RegexEvaluator::new(config)?),
        })
    }

    /// Register a new evaluator. Evaluators are consulted in registration
    /// order; register specialists before generalists.
    pub fn register(&mut self, evaluator: Box<dyn ClaimEvaluator>) {
        self.evaluators.push(evaluator);
    }

    /// Number of registered evaluators (excludes the fallback).
    pub fn len(&self) -> usize {
        self.evaluators.len()
    }

    pub fn is_empty(&self) -> bool {
        self.evaluators.is_empty()
    }

    /// Evaluate a claim against every constraint in `constraints`. For each
    /// constraint, the first registered evaluator that [`can_evaluate`] the
    /// pair owns it; otherwise the fallback (if configured) is used.
    ///
    /// Returns one [`EvalResult`] per constraint, in the same order as
    /// `constraints`.
    ///
    /// [`can_evaluate`]: ClaimEvaluator::can_evaluate
    pub fn evaluate_claim(&self, claim: &Claim, constraints: &[Constraint]) -> Vec<EvalResult> {
        let mut results = Vec::with_capacity(constraints.len());
        for constraint in constraints {
            let result = self
                .evaluators
                .iter()
                .find(|ev| ev.can_evaluate(claim, constraint))
                .map(|ev| ev.evaluate(claim, constraint))
                .or_else(|| {
                    self.fallback.as_ref().and_then(|fb| {
                        if fb.can_evaluate(claim, constraint) {
                            Some(fb.evaluate(claim, constraint))
                        } else {
                            None
                        }
                    })
                })
                .unwrap_or_else(|| {
                    EvalResult::allow(format!(
                        "no evaluator matched constraint {} (fail-open)",
                        constraint.id
                    ))
                });
            results.push(result);
        }
        results
    }

    /// Names of the registered evaluators (for logging).
    pub fn evaluator_names(&self) -> Vec<&str> {
        self.evaluators.iter().map(|e| e.name()).collect()
    }
}

impl Default for EvaluatorPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claim::ClaimType;
    use crate::constraint::{ConstraintsSection, EpistemicType};

    fn make_config(constraints: Vec<Constraint>) -> RigorConfig {
        RigorConfig {
            constraints: ConstraintsSection {
                beliefs: constraints,
                justifications: vec![],
                defeaters: vec![],
            },
            relations: vec![],
        }
    }

    fn make_claim(id: &str, text: &str) -> Claim {
        Claim {
            id: id.to_string(),
            text: text.to_string(),
            domain: None,
            confidence: 0.9,
            claim_type: ClaimType::Assertion,
            source_line: None,
            source: None,
        }
    }

    fn make_constraint(id: &str, rego: &str, tags: Vec<&str>) -> Constraint {
        Constraint {
            id: id.to_string(),
            epistemic_type: EpistemicType::Belief,
            name: id.to_string(),
            description: "test constraint".to_string(),
            rego: rego.to_string(),
            message: "violation".to_string(),
            tags: tags.into_iter().map(String::from).collect(),
            domain: None,
            references: vec![],
            source: vec![],
        }
    }

    #[test]
    fn regex_evaluator_flags_matching_claim() {
        let c = make_constraint(
            "no-unsafe",
            r#"
violation contains v if {
    some c in input.claims
    contains(c.text, "unsafe")
    v := {"constraint_id": "no-unsafe", "violated": true, "claims": [c.id], "reason": "Found unsafe claim"}
}
"#,
            vec![],
        );
        let config = make_config(vec![c.clone()]);
        let ev = RegexEvaluator::new(&config).unwrap();
        let claim = make_claim("c1", "This code is unsafe");
        let result = ev.evaluate(&claim, &c);
        assert!(result.violated, "expected violation, got {:?}", result);
    }

    #[test]
    fn regex_evaluator_allows_clean_claim() {
        let c = make_constraint(
            "no-unsafe",
            r#"
violation contains v if {
    some c in input.claims
    contains(c.text, "unsafe")
    v := {"constraint_id": "no-unsafe", "violated": true, "claims": [c.id], "reason": "Found unsafe claim"}
}
"#,
            vec![],
        );
        let config = make_config(vec![c.clone()]);
        let ev = RegexEvaluator::new(&config).unwrap();
        let claim = make_claim("c1", "This code is perfectly safe");
        let result = ev.evaluate(&claim, &c);
        assert!(!result.violated);
    }

    #[test]
    fn semantic_evaluator_routes_by_tag() {
        let tagged = make_constraint("sem", "", vec!["semantic"]);
        let untagged = make_constraint("reg", "violation contains v if { false; v := 0 }", vec![]);
        let claim = make_claim("c1", "text");
        let ev = SemanticEvaluator::new();
        assert!(ev.can_evaluate(&claim, &tagged));
        assert!(!ev.can_evaluate(&claim, &untagged));
    }

    #[test]
    fn semantic_evaluator_routes_empty_rego() {
        let empty = make_constraint("empty", "", vec![]);
        let claim = make_claim("c1", "text");
        let ev = SemanticEvaluator::new();
        assert!(ev.can_evaluate(&claim, &empty));
    }

    #[test]
    fn pipeline_routes_to_first_matching_evaluator() {
        let rego_c = make_constraint(
            "no-unsafe",
            r#"
violation contains v if {
    some c in input.claims
    contains(c.text, "unsafe")
    v := {"constraint_id": "no-unsafe", "violated": true, "claims": [c.id], "reason": "Found unsafe claim"}
}
"#,
            vec![],
        );
        let sem_c = make_constraint("sem", "", vec!["semantic"]);
        let config = make_config(vec![rego_c.clone(), sem_c.clone()]);
        let mut pipeline = EvaluatorPipeline::with_default_fallback(&config).unwrap();
        pipeline.register(Box::new(SemanticEvaluator::new()));
        pipeline.register(Box::new(RegexEvaluator::new(&config).unwrap()));

        let claim = make_claim("c1", "This code is unsafe");
        let results = pipeline.evaluate_claim(&claim, &[rego_c, sem_c]);
        assert_eq!(results.len(), 2);
        // Rego-matched claim: violated
        assert!(results[0].violated);
        // Semantic-routed constraint: deferred, not violated
        assert!(!results[1].violated);
        assert!(results[1].reason.contains("deferred"));
    }

    #[test]
    fn pipeline_falls_back_when_no_evaluator_matches() {
        // Pipeline with only the semantic evaluator; a rego-only constraint
        // should fall through to the default Rego fallback.
        let rego_c = make_constraint(
            "no-unsafe",
            r#"
violation contains v if {
    some c in input.claims
    contains(c.text, "unsafe")
    v := {"constraint_id": "no-unsafe", "violated": true, "claims": [c.id], "reason": "Found unsafe claim"}
}
"#,
            vec![],
        );
        let config = make_config(vec![rego_c.clone()]);
        let mut pipeline = EvaluatorPipeline::with_default_fallback(&config).unwrap();
        pipeline.register(Box::new(SemanticEvaluator::new()));

        let claim = make_claim("c1", "This code is unsafe");
        let results = pipeline.evaluate_claim(&claim, &[rego_c]);
        assert!(results[0].violated, "fallback should have caught it");
    }

    #[test]
    fn empty_pipeline_without_fallback_is_permissive() {
        let rego_c = make_constraint("c1", "violation contains v if { false; v := 0 }", vec![]);
        let pipeline = EvaluatorPipeline::new();
        let claim = make_claim("c1", "text");
        let results = pipeline.evaluate_claim(&claim, &[rego_c]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].violated);
        assert!(results[0].reason.contains("no evaluator matched"));
    }
}
