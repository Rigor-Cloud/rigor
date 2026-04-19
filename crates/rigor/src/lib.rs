pub mod alerting;
pub mod claim;
pub mod cli;
pub mod config;
pub mod constraint;
pub mod daemon;
pub mod defaults;
pub mod hook;
pub mod logging;
pub mod lsp;
pub mod memory;
pub mod observability;
pub mod policy;
pub mod violation;
pub mod fallback;

use anyhow::Result;
use std::path::Path;
use tracing::{debug, info, info_span, warn};

use claim::{Claim, ClaimExtractor, HeuristicExtractor};
use config::find_rigor_lock;
use config::find_rigor_yaml;
use constraint::graph::ArgumentationGraph;
use hook::{HookResponse, StopHookInput};
use policy::{EvaluationInput, PolicyEngine};
use violation::{
    collect_violations, determine_decision, Decision, SeverityThresholds, ViolationFormatter,
};

/// Main entry point for the Rigor stop hook.
/// Reads input from stdin, evaluates constraints, returns JSON response to stdout.
pub fn run() -> Result<()> {
    // Initialize observability first (before any other work)
    observability::init_tracing()?;

    let result = run_hook();

    // Shutdown OTEL (flush pending spans)
    observability::shutdown();

    result
}

fn run_hook() -> Result<()> {
    let span = info_span!("rigor_hook");
    let _guard = span.enter();

    // No-op if no rigor daemon is running. Without rigor-personal active,
    // claim extraction's LLM-as-judge path has no captured API key, no
    // dashboard to stream to, and nothing to do. Return allow silently so
    // the session behaves exactly as if rigor weren't installed at all.
    if !daemon::daemon_alive() {
        // Still drain stdin (Claude Code expects the hook to consume its
        // input) and emit the allow response. Nothing else runs.
        let _ = StopHookInput::from_stdin();
        HookResponse::allow().write_stdout()?;
        return Ok(());
    }

    // Read hook input from stdin
    let input = StopHookInput::from_stdin()?;

    info!(
        session_id = %input.session_id,
        stop_hook_active = input.stop_hook_active,
        "Hook invoked"
    );

    // CRITICAL: Check stop_hook_active to prevent infinite loops
    if input.stop_hook_active {
        let response = HookResponse::allow();
        response.write_stdout()?;
        warn!("stop_hook_active=true, allowing to prevent loop");
        return Ok(());
    }

    // Look for rigor.yaml (constraint config) or rigor.lock (legacy)
    let yaml_path = find_rigor_yaml();
    let lock_path = find_rigor_lock();

    if yaml_path.is_none() && lock_path.is_none() {
        // No config = no constraints = always allow
        let response = HookResponse::allow();
        response.write_stdout()?;
        info!("No rigor.yaml or rigor.lock found, allowing");
        return Ok(());
    }

    // If rigor.yaml exists, evaluate constraint pipeline
    if let Some(yaml_path) = yaml_path {
        info!(config = %yaml_path.display(), "Found rigor.yaml");
        return evaluate_constraints(&yaml_path, &input.transcript_path);
    }

    // rigor.lock exists but no rigor.yaml — allow with status
    let config_path = lock_path.unwrap();
    info!(config = %config_path.display(), "Found rigor.lock (no rigor.yaml)");
    let response = HookResponse::allow();
    response.write_stdout()?;
    eprintln!("rigor: 0 constraints, 0 violations");

    Ok(())
}

/// Evaluate the constraint pipeline from a rigor.yaml file.
/// Every step that can fail uses the fail-open pattern.
fn evaluate_constraints(yaml_path: &Path, transcript_path: &str) -> Result<()> {
    // Step 0: Capture session metadata at the very start
    let session = logging::SessionMetadata::capture();

    // Step 1: Load rigor.yaml
    let config = match constraint::loader::load_rigor_config(yaml_path) {
        Ok(config) => config,
        Err(e) => {
            warn!(error = %e, "Failed to load rigor.yaml, failing open");
            let response = HookResponse::allow();
            response.write_stdout()?;
            return Ok(());
        }
    };

    let constraint_count = config.all_constraints().len();

    // Step 2: Build argumentation graph and compute strengths
    let mut graph = ArgumentationGraph::from_config(&config);
    if let Err(e) = graph.compute_strengths() {
        warn!(error = %e, "Failed to compute constraint strengths, failing open");
        let response = HookResponse::allow();
        response.write_stdout()?;
        return Ok(());
    }
    let strengths = graph.get_all_strengths();

    // Step 3: Create policy engine
    let mut engine = match PolicyEngine::new(&config) {
        Ok(engine) => engine,
        Err(e) => {
            warn!(error = %e, "Failed to create policy engine, failing open");
            let response = HookResponse::allow();
            response.write_stdout()?;
            return Ok(());
        }
    };

    // Step 4: Extract claims from transcript (or use test claims if env var set)
    let claims = match std::env::var("RIGOR_TEST_CLAIMS") {
        Ok(json_str) => {
            // RIGOR_TEST_CLAIMS overrides transcript extraction (for testing)
            match serde_json::from_str::<Vec<Claim>>(&json_str) {
                Ok(claims) => {
                    info!(
                        count = claims.len(),
                        "Loaded test claims from RIGOR_TEST_CLAIMS"
                    );
                    claims
                }
                Err(e) => {
                    warn!(error = %e, "Failed to parse RIGOR_TEST_CLAIMS, falling back to transcript");
                    extract_claims_from_transcript(Path::new(transcript_path))?
                }
            }
        }
        Err(_) => {
            // Normal operation: extract from transcript
            extract_claims_from_transcript(Path::new(transcript_path))?
        }
    };

    // Debug claim visualization (if RIGOR_DEBUG is set)
    if std::env::var("RIGOR_DEBUG").is_ok() {
        debug!("Extracted claims:");
        for (i, claim) in claims.iter().enumerate() {
            debug!(
                claim_num = i + 1,
                text = %claim.text,
                confidence = claim.confidence,
                claim_type = ?claim.claim_type,
                "Claim"
            );
        }
    }

    let eval_input = EvaluationInput {
        claims: claims.clone(),
    };

    // Step 5: Evaluate policies
    let raw_violations = match engine.evaluate(&eval_input) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "Failed to evaluate policies, failing open");
            let response = HookResponse::allow();
            response.write_stdout()?;
            return Ok(());
        }
    };

    // Step 6: Build constraint metadata map
    let constraint_meta: std::collections::HashMap<String, violation::ConstraintMeta> = config
        .all_constraints()
        .iter()
        .map(|c| {
            let epistemic_type = match c.epistemic_type {
                constraint::types::EpistemicType::Belief => "belief",
                constraint::types::EpistemicType::Justification => "justification",
                constraint::types::EpistemicType::Defeater => "defeater",
            };
            (
                c.id.clone(),
                violation::ConstraintMeta {
                    name: c.name.clone(),
                    epistemic_type: epistemic_type.to_string(),
                    rego_path: format!("data.rigor.{}", c.id),
                },
            )
        })
        .collect();

    // Step 6.5: Collect violations with severity
    let thresholds = SeverityThresholds::default();
    let violations = collect_violations(
        raw_violations,
        &strengths,
        &thresholds,
        &constraint_meta,
        &claims,
    );
    let violation_count = violations.len();

    // Step 7: Determine decision
    let decision = determine_decision(&violations);

    // Step 7.5: Log violations (fail-open on logging errors)
    if !violations.is_empty() {
        match logging::ViolationLogger::new() {
            Ok(logger) => {
                for violation in &violations {
                    // Get base strength from graph
                    let base_strength = graph
                        .nodes()
                        .get(&violation.constraint_id)
                        .map(|node| node.base_strength)
                        .unwrap_or(0.8);

                    // Get supporters and attackers for this constraint
                    let supporters: Vec<String> = graph
                        .relations()
                        .iter()
                        .filter(|r| {
                            r.to == violation.constraint_id
                                && r.relation_type == constraint::types::RelationType::Supports
                        })
                        .map(|r| r.from.clone())
                        .collect();

                    let attackers: Vec<String> = graph
                        .relations()
                        .iter()
                        .filter(|r| {
                            r.to == violation.constraint_id
                                && (r.relation_type == constraint::types::RelationType::Attacks
                                    || r.relation_type
                                        == constraint::types::RelationType::Undercuts)
                        })
                        .map(|r| r.from.clone())
                        .collect();

                    // Map severity to string
                    let severity_str = match violation.severity {
                        violation::Severity::Block => "block",
                        violation::Severity::Warn => "warn",
                        violation::Severity::Allow => "allow",
                    };

                    // Map decision to string
                    let decision_str = match &decision {
                        Decision::Block { .. } => "block",
                        Decision::Warn { .. } | Decision::Allow => "allow",
                    };

                    // Look up the source claim for provenance
                    let source_claim = violation
                        .claim_ids
                        .first()
                        .and_then(|cid| claims.iter().find(|c| &c.id == cid));

                    let claim_confidence = source_claim.map(|c| c.confidence);
                    let claim_type_str = source_claim.map(|c| format!("{:?}", c.claim_type).to_lowercase());
                    let claim_source = source_claim
                        .and_then(|c| c.source.as_ref())
                        .map(|s| logging::ClaimSource {
                            message_index: s.message_index,
                            sentence_index: s.sentence_index,
                        });

                    let entry = logging::ViolationLogEntry {
                        session: session.clone(),
                        constraint_id: violation.constraint_id.clone(),
                        constraint_name: violation.constraint_name.clone(),
                        claim_ids: violation.claim_ids.clone(),
                        claim_text: violation.claim_text.clone(),
                        base_strength,
                        computed_strength: violation.strength,
                        severity: severity_str.to_string(),
                        decision: decision_str.to_string(),
                        message: violation.message.clone(),
                        supporters,
                        attackers,
                        total_claims: claims.len(),
                        total_constraints: constraint_count,
                        transcript_path: Some(transcript_path.to_string()),
                        claim_confidence,
                        claim_type: claim_type_str,
                        claim_source,
                        false_positive: None,
                        annotation_note: None,
                        model: None,
                    };

                    if let Err(e) = logger.log(&entry) {
                        warn!(error = %e, "Failed to log violation, continuing");
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to create ViolationLogger, continuing without logging");
            }
        }
    }

    // Step 8: Map decision to hook response
    let formatter = ViolationFormatter::new();
    let response = match &decision {
        Decision::Block { violations } => {
            let reason = formatter.format_violations(violations);
            info!(reason = %reason, "Blocking due to constraint violations");
            let mut resp = HookResponse::block(reason);
            resp.metadata.constraint_count = constraint_count;
            resp.metadata.claim_count = claims.len();
            resp
        }
        Decision::Warn { violations } => {
            let reason = formatter.format_violations(violations);
            info!(reason = %reason, "Warning about constraint violations");
            let mut resp = HookResponse::allow();
            resp.reason = Some(format!("rigor warning: {}", reason));
            resp.metadata.constraint_count = constraint_count;
            resp.metadata.claim_count = claims.len();
            resp
        }
        Decision::Allow => {
            let mut resp = HookResponse::allow();
            resp.metadata.constraint_count = constraint_count;
            resp.metadata.claim_count = claims.len();
            resp
        }
    };

    response.write_stdout()?;
    eprintln!(
        "rigor: {} constraints, {} violations",
        constraint_count, violation_count
    );

    Ok(())
}

/// Extract claims from a transcript file using heuristic extraction.
/// Returns fallback claims if extraction fails (fail-open).
fn extract_claims_from_transcript(path: &Path) -> Result<Vec<Claim>> {
    // Parse transcript
    let messages = match claim::transcript::parse_transcript(path) {
        Ok(msgs) => msgs,
        Err(e) => {
            warn!(error = %e, path = %path.display(), "Failed to parse transcript, using empty claims");
            return Ok(Vec::new());
        }
    };

    // Extract claims using heuristic extractor
    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);

    info!(count = claims.len(), "Extracted claims from transcript");

    Ok(claims)
}
