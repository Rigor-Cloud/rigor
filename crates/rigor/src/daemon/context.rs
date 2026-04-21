use crate::constraint::graph::ArgumentationGraph;
use crate::constraint::types::{EpistemicType, RigorConfig};
use crate::logging::ViolationLogger;
use crate::memory::MemoryStore;

/// Build epistemic context to inject into the AI's system prompt.
/// This teaches the AI the verified truths so it internalizes them
/// BEFORE generating any response — not just as constraints to avoid
/// violating, but as facts it should believe and use.
pub fn build_epistemic_context(config: &RigorConfig, graph: &ArgumentationGraph) -> String {
    let mut ctx = String::new();
    ctx.push_str("\n\n[RIGOR EPISTEMIC GROUNDING]\n");
    ctx.push_str("You are epistemically grounded. The following are VERIFIED TRUTHS.\n");
    ctx.push_str("Internalize these as facts. Do not contradict them under any circumstances.\n");
    ctx.push_str("Do not state falsehoods even as examples, tests, quotes, or demonstrations.\n\n");

    // Teach the truths grouped by epistemic type
    let beliefs: Vec<_> = config
        .all_constraints()
        .into_iter()
        .filter(|c| c.epistemic_type == EpistemicType::Belief)
        .collect();

    if !beliefs.is_empty() {
        ctx.push_str("VERIFIED TRUTHS:\n");
        for c in &beliefs {
            let strength = graph.get_strength(&c.id).unwrap_or(0.0);
            ctx.push_str(&format!(
                "• {} — {} [strength: {:.2}]\n",
                c.name, c.description, strength
            ));
        }
        ctx.push('\n');
    }

    // Justifications as evidence requirements
    let justifications: Vec<_> = config
        .all_constraints()
        .into_iter()
        .filter(|c| c.epistemic_type == EpistemicType::Justification)
        .collect();

    if !justifications.is_empty() {
        ctx.push_str("EVIDENCE REQUIREMENTS:\n");
        for c in &justifications {
            ctx.push_str(&format!("• {} — {}\n", c.name, c.description));
        }
        ctx.push('\n');
    }

    // Defeaters as known contradictions
    let defeaters: Vec<_> = config
        .all_constraints()
        .into_iter()
        .filter(|c| c.epistemic_type == EpistemicType::Defeater)
        .collect();

    if !defeaters.is_empty() {
        ctx.push_str("KNOWN CONTRADICTIONS:\n");
        for c in &defeaters {
            ctx.push_str(&format!("• {} — {}\n", c.name, c.description));
        }
        ctx.push('\n');
    }

    // Recent violations — teach from past mistakes in this session
    if let Ok(logger) = ViolationLogger::new() {
        if let Ok(entries) = logger.read_all() {
            let recent: Vec<_> = entries.iter().rev().take(5).collect();
            if !recent.is_empty() {
                ctx.push_str("PAST VIOLATIONS IN THIS SESSION (do not repeat these errors):\n");
                for entry in recent {
                    ctx.push_str(&format!(
                        "• You previously said: \"{}\" — this was WRONG because: {}\n",
                        entry.claim_text.first().unwrap_or(&String::new()),
                        entry.message
                    ));
                }
                ctx.push('\n');
            }
        }
    }

    // Cross-session memory: episodic + semantic. We rebuild deterministically
    // from the violation log on every context build so it's always in sync.
    // If the log is missing/empty, both sections are silently skipped.
    if let Ok(memory) = MemoryStore::rebuild_from_log() {
        let recent = memory.recent_episodes(3);
        let non_fp: Vec<_> = recent
            .iter()
            .filter(|e| e.outcome != "false_positive_dominant")
            .collect();
        if !non_fp.is_empty() {
            ctx.push_str(
                "RELEVANT PAST EPISODES (from previous sessions — do not repeat these errors):\n",
            );
            for ep in non_fp.iter().take(3) {
                let sid_short = &ep.session_id[..ep.session_id.len().min(8)];
                ctx.push_str(&format!(
                    "• session {} ({}): {} violation(s), constraints: {}\n",
                    sid_short,
                    ep.timestamp,
                    ep.total_violations,
                    ep.constraint_ids.join(", ")
                ));
                for s in ep.sample_claims.iter().take(2) {
                    ctx.push_str(&format!("   - flagged claim: \"{}\"\n", s));
                }
            }
            ctx.push('\n');
        }

        let warnings = memory.model_warnings();
        if !warnings.is_empty() {
            ctx.push_str("MODEL-SPECIFIC WARNINGS (learned from history):\n");
            for w in &warnings {
                ctx.push_str(&format!("• {}\n", w));
            }
            ctx.push('\n');
        }

        let top = memory.top_relevant_constraints(5);
        if !top.is_empty() {
            ctx.push_str(
                "HIGH-RISK CONSTRAINT CATEGORIES (most frequently triggered historically):\n",
            );
            for (cid, n) in &top {
                ctx.push_str(&format!("• {} — fired in {} prior session(s)\n", cid, n));
            }
            ctx.push('\n');
        }
    }

    ctx.push_str("Your claims are being evaluated. Violations will block your response.\n");
    ctx.push_str("[END RIGOR EPISTEMIC GROUNDING]\n");
    ctx
}
