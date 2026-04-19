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
//! - [`SemanticEvaluator`] — the LLM-as-judge path. Reads verdicts from a
//!   [`RelevanceLookup`] (which in turn reads
//!   `daemon::proxy::score_claim_relevance`'s cache, either in-process or
//!   over HTTP). A `high`/`medium` cache hit for (claim_text, constraint_id)
//!   produces a violation; a miss is fail-open allow (the judge hasn't run
//!   yet, or returned `low`).
//!
//! The routing contract is intentionally simple: the pipeline asks each
//! evaluator in registration order whether it [`can_evaluate`] the pair, and
//! uses the first match. If nothing matches, the pipeline falls back to the
//! default Rego evaluator so existing behaviour is preserved.

use std::sync::Arc;

use crate::claim::Claim;
use crate::constraint::{Constraint, RigorConfig};
use crate::evaluator::relevance::RelevanceLookup;
use crate::policy::{EvaluationInput, PolicyEngine, RawViolation};

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

    /// Construct from a pre-built [`PolicyEngine`]. Use this on hot paths
    /// (the daemon caches a compiled engine in `DaemonState`) so we avoid
    /// reparsing Rego for every request.
    pub fn from_engine(engine: PolicyEngine) -> Self {
        Self { engine }
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
/// Backed by a [`RelevanceLookup`] that returns cached verdicts produced by
/// `daemon::proxy::score_claim_relevance`. Two lookup implementations ship:
/// [`crate::evaluator::relevance::InProcessLookup`] for the daemon and
/// [`crate::evaluator::relevance::HttpLookup`] for the stop-hook subprocess.
///
/// Verdict semantics:
///
/// - Cache hit for (claim.text, constraint.id) with relevance `"high"` or
///   `"medium"` → [`EvalResult::violation`] carrying the judge's reason.
/// - Cache miss → [`EvalResult::allow`]. This is fail-open: the judge may
///   simply not have scored this claim yet, or may have returned `"low"`
///   (which the daemon never caches). Either way we must not block.
///
/// The synchronous `evaluate` contract is preserved — the judge call itself
/// runs asynchronously in the daemon; this evaluator only reads the cache
/// populated by that async pass.
pub struct SemanticEvaluator {
    /// Case-insensitive tag that marks a constraint as semantic.
    tag: String,
    lookup: Arc<dyn RelevanceLookup>,
}

impl SemanticEvaluator {
    /// Construct with the default `"semantic"` routing tag.
    pub fn new(lookup: Arc<dyn RelevanceLookup>) -> Self {
        Self {
            tag: "semantic".to_string(),
            lookup,
        }
    }

    /// Construct with a custom routing tag.
    pub fn with_tag(tag: impl Into<String>, lookup: Arc<dyn RelevanceLookup>) -> Self {
        Self {
            tag: tag.into(),
            lookup,
        }
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

    fn evaluate(&self, claim: &Claim, constraint: &Constraint) -> EvalResult {
        let matches = self.lookup.lookup(claim);
        if let Some(m) = matches.iter().find(|m| m.constraint_id == constraint.id) {
            // The judge produced a high/medium match. Surface it as a
            // violation, preferring the judge's reason when present.
            let reason = if m.reason.trim().is_empty() {
                format!(
                    "semantic match ({}) on constraint {}",
                    m.relevance, constraint.id
                )
            } else {
                m.reason.clone()
            };
            EvalResult::violation(reason, claim.confidence)
        } else {
            // No verdict available: fail-open. `collect_violations` will
            // never see a RawViolation for this pair.
            EvalResult::allow(format!(
                "no semantic verdict for {} (not scored or ranked low)",
                constraint.id
            ))
        }
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

    /// Pipeline seeded with a fallback built from a pre-compiled engine.
    /// Cheaper than [`with_default_fallback`] on hot paths — avoids
    /// reparsing Rego when the caller already has a cached
    /// [`PolicyEngine`] (e.g. `DaemonState::policy_engine`).
    pub fn with_engine_fallback(engine: PolicyEngine) -> Self {
        Self {
            evaluators: Vec::new(),
            fallback: Some(RegexEvaluator::from_engine(engine)),
        }
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

    /// Evaluate every claim against every constraint and collapse the
    /// results into the same [`RawViolation`] shape that
    /// [`crate::policy::PolicyEngine::evaluate`] produced. This is the
    /// drop-in replacement call sites (both `lib.rs` and
    /// `daemon/proxy.rs`) use so the downstream severity/decision path
    /// (`collect_violations` → `determine_decision`) stays unchanged.
    ///
    /// One `RawViolation` is emitted per (claim, constraint) pair that
    /// the pipeline's evaluators flagged as violated, matching the
    /// per-claim, per-constraint granularity of the previous pipeline.
    pub fn run(&self, claims: &[Claim], constraints: &[Constraint]) -> Vec<RawViolation> {
        let mut raw_violations = Vec::new();
        for claim in claims {
            let results = self.evaluate_claim(claim, constraints);
            for (constraint, result) in constraints.iter().zip(results.iter()) {
                if result.violated {
                    raw_violations.push(RawViolation {
                        constraint_id: constraint.id.clone(),
                        violated: true,
                        claims: vec![claim.id.clone()],
                        reason: result.reason.clone(),
                    });
                }
            }
        }
        raw_violations
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
    use crate::evaluator::relevance::{RelevanceLookup, RelevanceMatch};

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

    /// Test-only lookup: returns a fixed set of matches regardless of the
    /// claim, so tests can assert SemanticEvaluator's routing + verdict
    /// behaviour without a live daemon.
    struct FixedLookup(Vec<RelevanceMatch>);

    impl RelevanceLookup for FixedLookup {
        fn lookup(&self, _claim: &Claim) -> Vec<RelevanceMatch> {
            self.0.clone()
        }
    }

    /// Test-only lookup that always returns nothing.
    struct EmptyLookup;

    impl RelevanceLookup for EmptyLookup {
        fn lookup(&self, _claim: &Claim) -> Vec<RelevanceMatch> {
            Vec::new()
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
        let ev = SemanticEvaluator::new(Arc::new(EmptyLookup));
        assert!(ev.can_evaluate(&claim, &tagged));
        assert!(!ev.can_evaluate(&claim, &untagged));
    }

    #[test]
    fn semantic_evaluator_routes_empty_rego() {
        let empty = make_constraint("empty", "", vec![]);
        let claim = make_claim("c1", "text");
        let ev = SemanticEvaluator::new(Arc::new(EmptyLookup));
        assert!(ev.can_evaluate(&claim, &empty));
    }

    #[test]
    fn semantic_evaluator_flags_when_cache_has_match() {
        let constraint = make_constraint("sem-claim", "", vec!["semantic"]);
        let claim = make_claim("c1", "Rust has garbage collection");
        let lookup = FixedLookup(vec![RelevanceMatch {
            constraint_id: "sem-claim".to_string(),
            relevance: "high".to_string(),
            reason: "Asserts GC in Rust".to_string(),
        }]);
        let ev = SemanticEvaluator::new(Arc::new(lookup));
        let result = ev.evaluate(&claim, &constraint);
        assert!(result.violated, "expected violation from semantic match");
        assert!(
            result.reason.contains("GC in Rust"),
            "expected judge reason, got {:?}",
            result.reason
        );
    }

    #[test]
    fn semantic_evaluator_allows_when_cache_miss() {
        let constraint = make_constraint("sem-claim", "", vec!["semantic"]);
        let claim = make_claim("c1", "some unrelated claim");
        let ev = SemanticEvaluator::new(Arc::new(EmptyLookup));
        let result = ev.evaluate(&claim, &constraint);
        assert!(!result.violated, "cache miss must fail-open");
    }

    #[test]
    fn semantic_evaluator_ignores_unrelated_cache_entries() {
        // The cache may contain matches for OTHER constraints on the same
        // claim text — we must not cross-attribute them.
        let target = make_constraint("sem-target", "", vec!["semantic"]);
        let claim = make_claim("c1", "multi-link claim");
        let lookup = FixedLookup(vec![RelevanceMatch {
            constraint_id: "some-other-constraint".to_string(),
            relevance: "high".to_string(),
            reason: "different constraint".to_string(),
        }]);
        let ev = SemanticEvaluator::new(Arc::new(lookup));
        let result = ev.evaluate(&claim, &target);
        assert!(
            !result.violated,
            "must only violate when cache has a match for THIS constraint"
        );
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
        pipeline.register(Box::new(SemanticEvaluator::new(Arc::new(EmptyLookup))));
        pipeline.register(Box::new(RegexEvaluator::new(&config).unwrap()));

        let claim = make_claim("c1", "This code is unsafe");
        let results = pipeline.evaluate_claim(&claim, &[rego_c, sem_c]);
        assert_eq!(results.len(), 2);
        // Rego-matched claim: violated
        assert!(results[0].violated);
        // Semantic-routed constraint with empty cache: not violated (fail-open)
        assert!(!results[1].violated);
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
        pipeline.register(Box::new(SemanticEvaluator::new(Arc::new(EmptyLookup))));

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
