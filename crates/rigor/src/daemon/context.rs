use crate::constraint::graph::ArgumentationGraph;
use crate::constraint::types::{EpistemicType, RigorConfig};
use crate::logging::ViolationLogger;

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
    let beliefs: Vec<_> = config.all_constraints().into_iter()
        .filter(|c| c.epistemic_type == EpistemicType::Belief)
        .collect();

    if !beliefs.is_empty() {
        ctx.push_str("VERIFIED TRUTHS:\n");
        for c in &beliefs {
            let strength = graph.get_strength(&c.id).unwrap_or(0.0);
            ctx.push_str(&format!("• {} — {} [strength: {:.2}]\n", c.name, c.description, strength));
        }
        ctx.push_str("\n");
    }

    // Justifications as evidence requirements
    let justifications: Vec<_> = config.all_constraints().into_iter()
        .filter(|c| c.epistemic_type == EpistemicType::Justification)
        .collect();

    if !justifications.is_empty() {
        ctx.push_str("EVIDENCE REQUIREMENTS:\n");
        for c in &justifications {
            ctx.push_str(&format!("• {} — {}\n", c.name, c.description));
        }
        ctx.push_str("\n");
    }

    // Defeaters as known contradictions
    let defeaters: Vec<_> = config.all_constraints().into_iter()
        .filter(|c| c.epistemic_type == EpistemicType::Defeater)
        .collect();

    if !defeaters.is_empty() {
        ctx.push_str("KNOWN CONTRADICTIONS:\n");
        for c in &defeaters {
            ctx.push_str(&format!("• {} — {}\n", c.name, c.description));
        }
        ctx.push_str("\n");
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
                ctx.push_str("\n");
            }
        }
    }

    ctx.push_str("Your claims are being evaluated. Violations will block your response.\n");
    ctx.push_str("[END RIGOR EPISTEMIC GROUNDING]\n");
    ctx
}
