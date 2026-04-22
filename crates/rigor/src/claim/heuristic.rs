use once_cell::sync::Lazy;
use regex::Regex;
/// Heuristic claim extraction from natural language text.
///
/// Extracts sentence-level claims using:
/// - Sentence segmentation (unicode-segmentation)
/// - Assertion filtering (questions, hypotheticals, code)
/// - Hedge detection (filter uncertain statements)
/// - Claim type classification
/// - Confidence scoring
use unicode_segmentation::UnicodeSegmentation;

use crate::claim::confidence::assign_confidence;
use crate::claim::hedge_detector::is_hedged;
use crate::claim::types::{Claim, ClaimType, SourceLocation};

static CODE_BLOCK_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"```[\s\S]*?```").expect("Valid code block pattern"));

static HYPOTHETICAL_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(if|when|suppose|assuming|what if)\b").expect("Valid hypothetical pattern")
});

static ACTION_INTENT_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(let me|i'?ll|i'?m going to|i will|i should|i need to|i'?d|we need to|we should|this (should|needs to) be|that (should|needs to) be)\b").expect("Valid action intent pattern")
});

static ACTION_VERB_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(edit|modif|chang|fix|remov|add|creat|refactor|updat|delet|rewrit|install|configur|implement|writ|replac|renam|mov)").expect("Valid action verb pattern")
});

/// Strip fenced code blocks from text.
pub fn strip_code_blocks(text: &str) -> String {
    CODE_BLOCK_PATTERN.replace_all(text, "").to_string()
}

/// Returns true if the sentence appears to propose an action by the AI.
/// Requires both an action-intent opener AND an action verb.
pub fn is_action_intent(sentence: &str) -> bool {
    let trimmed = sentence.trim();
    if trimmed.is_empty() || trimmed.ends_with('?') {
        return false;
    }
    ACTION_INTENT_PATTERN.is_match(trimmed) && ACTION_VERB_PATTERN.is_match(trimmed)
}

/// Check if a sentence is an assertion (not a question, hypothetical, or code).
pub fn is_assertion(sentence: &str) -> bool {
    let trimmed = sentence.trim();

    if trimmed.is_empty() {
        return false;
    }

    // Filter questions
    if trimmed.ends_with('?') {
        return false;
    }

    // Filter hypotheticals
    if HYPOTHETICAL_PATTERN.is_match(trimmed) {
        return false;
    }

    // Filter code lines
    if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("```") {
        return false;
    }

    // Filter conversational/greeting text — these are not factual claims
    let lower = trimmed.to_lowercase();
    let conversational_starts = [
        "hi",
        "hello",
        "hey",
        "sure",
        "ok",
        "okay",
        "got it",
        "let me",
        "i can",
        "i'll",
        "i'd",
        "how can i",
        "what would you",
        "thanks",
        "thank you",
        "you're welcome",
        "no problem",
        "great",
        "awesome",
        "sounds good",
        "absolutely",
        "of course",
        "happy to",
        "glad to",
        "here's",
        "here is",
        "let's",
        "shall we",
        "would you like",
        "is there anything",
        "what can i",
        "how about",
        "feel free",
    ];
    for prefix in &conversational_starts {
        if lower.starts_with(prefix) {
            return false;
        }
    }

    // Filter single-word lines — not claims
    if trimmed.split_whitespace().count() < 2 {
        return false;
    }

    // Filter emoji-heavy or exclamation-only lines
    if trimmed.chars().filter(|c| c.is_alphabetic()).count() < trimmed.len() / 2 {
        return false;
    }

    true
}

/// Classify claim type based on sentence content.
pub fn classify_claim_type(sentence: &str) -> ClaimType {
    let lower = sentence.to_lowercase();

    // Check negation first
    if lower.contains(" not ")
        || lower.contains(" doesn't ")
        || lower.contains(" don't ")
        || lower.contains(" cannot ")
        || lower.contains(" can't ")
    {
        return ClaimType::Negation;
    }

    // Check for dependency claims (crate/library/package/version references)
    if lower.contains(" crate ")
        || lower.contains(" library ")
        || lower.contains(" package ")
        || lower.contains(" dependency ")
        || lower.contains(" version ")
    {
        return ClaimType::DependencyClaim;
    }

    // Check for architectural decisions
    if lower.contains(" architecture ")
        || lower.contains(" pattern ")
        || lower.contains(" design ")
        || lower.contains(" should use ")
        || lower.contains(" approach ")
        || lower.contains(" module ")
        || lower.contains(" layer ")
    {
        return ClaimType::ArchitecturalDecision;
    }

    // Check for code references
    if lower.contains("::")
        || lower.contains("fn ")
        || lower.contains("struct ")
        || lower.contains("impl ")
        || lower.contains(".rs")
        || lower.contains("()")
    {
        return ClaimType::CodeReference;
    }

    ClaimType::Assertion
}

/// Extract claims from text with source location tracking.
///
/// Process:
/// 1. Strip code blocks
/// 2. Segment into sentences
/// 3. Filter for assertions (not questions/hypotheticals/code)
/// 4. Filter hedged statements
/// 5. Assign confidence and classify type
pub fn extract_claims_from_text(text: &str, message_index: usize) -> Vec<Claim> {
    let cleaned_text = strip_code_blocks(text);

    cleaned_text
        .unicode_sentences()
        .enumerate()
        .filter(|(_, s)| is_assertion(s))
        .filter(|(_, s)| !is_hedged(s))
        .map(|(sentence_index, s)| {
            let text = s.trim().to_string();
            let confidence = assign_confidence(&text);
            let claim_type = if is_action_intent(&text) {
                ClaimType::ActionIntent
            } else {
                classify_claim_type(&text)
            };

            Claim::new(
                text,
                confidence,
                claim_type,
                Some(SourceLocation {
                    message_index,
                    sentence_index,
                }),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_code_blocks() {
        let input = "This is text.\n```rust\nfn main() {}\n```\nMore text.";
        let result = strip_code_blocks(input);
        assert!(!result.contains("fn main"));
        assert!(result.contains("This is text"));
        assert!(result.contains("More text"));
    }

    #[test]
    fn test_is_assertion_question() {
        assert!(!is_assertion("Does X support Y?"));
    }

    #[test]
    fn test_is_assertion_hypothetical() {
        assert!(!is_assertion("If X then Y"));
        assert!(!is_assertion("Suppose X is true"));
    }

    #[test]
    fn test_is_assertion_code() {
        assert!(!is_assertion("// This is a comment"));
        assert!(!is_assertion("# Python comment"));
        assert!(!is_assertion("```code```"));
    }

    #[test]
    fn test_is_assertion_valid() {
        assert!(is_assertion("X supports Y."));
        assert!(is_assertion("This library works."));
    }

    #[test]
    fn test_classify_negation() {
        assert_eq!(
            classify_claim_type("X does not support Y"),
            ClaimType::Negation
        );
        assert_eq!(classify_claim_type("X doesn't work"), ClaimType::Negation);
        assert_eq!(classify_claim_type("X cannot do Y"), ClaimType::Negation);
    }

    #[test]
    fn test_classify_assertion() {
        assert_eq!(classify_claim_type("X supports Y"), ClaimType::Assertion);
        assert_eq!(classify_claim_type("This works"), ClaimType::Assertion);
    }

    #[test]
    fn test_classify_dependency_claim() {
        assert_eq!(
            classify_claim_type("The regex crate supports Unicode"),
            ClaimType::DependencyClaim
        );
        assert_eq!(
            classify_claim_type("This library is fast"),
            ClaimType::DependencyClaim
        );
        assert_eq!(
            classify_claim_type("The package includes async support"),
            ClaimType::DependencyClaim
        );
    }

    #[test]
    fn test_classify_architectural_decision() {
        assert_eq!(
            classify_claim_type("The architecture uses a pipeline pattern"),
            ClaimType::ArchitecturalDecision
        );
        assert_eq!(
            classify_claim_type("This module handles claim extraction"),
            ClaimType::ArchitecturalDecision
        );
        assert_eq!(
            classify_claim_type("We should use a layered design approach"),
            ClaimType::ArchitecturalDecision
        );
    }

    #[test]
    fn test_classify_code_reference() {
        assert_eq!(
            classify_claim_type("The PolicyEngine::new() function creates an engine"),
            ClaimType::CodeReference
        );
        assert_eq!(
            classify_claim_type("The struct Claim has a text field"),
            ClaimType::CodeReference
        );
        assert_eq!(
            classify_claim_type("Check src/lib.rs for the entry point"),
            ClaimType::CodeReference
        );
    }

    #[test]
    fn test_extract_full_pipeline() {
        let text = "This library supports async. I think it might also support sync. Does it support streams?";
        let claims = extract_claims_from_text(text, 0);

        // Only "This library supports async." should be extracted
        assert_eq!(claims.len(), 1);
        assert!(claims[0].text.contains("async"));
        assert_eq!(claims[0].confidence, 0.7); // default, no definitive marker
        assert_eq!(claims[0].claim_type, ClaimType::DependencyClaim);
    }

    #[test]
    fn test_extract_filters_hedged() {
        let text = "X is Y. Maybe Z is W.";
        let claims = extract_claims_from_text(text, 0);

        assert_eq!(claims.len(), 1);
        assert!(claims[0].text.contains("X is Y"));
    }

    #[test]
    fn test_extract_strips_code() {
        let text = "This works.\n```rust\nfn test() {}\n```\nThat also works.";
        let claims = extract_claims_from_text(text, 0);

        assert_eq!(claims.len(), 2);
        assert!(claims[0].text.contains("This works"));
        assert!(claims[1].text.contains("That also works"));
    }

    #[test]
    fn test_extract_source_location() {
        let text = "First claim. Second claim.";
        let claims = extract_claims_from_text(text, 5);

        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].source.as_ref().unwrap().message_index, 5);
        assert_eq!(claims[0].source.as_ref().unwrap().sentence_index, 0);
        assert_eq!(claims[1].source.as_ref().unwrap().sentence_index, 1);
    }

    #[test]
    fn test_is_action_intent_detects_explicit_edits() {
        assert!(is_action_intent(
            "Let me edit src/claim/heuristic.rs to add the filter"
        ));
        assert!(is_action_intent(
            "I'll modify the proxy to handle this case"
        ));
    }

    #[test]
    fn test_is_action_intent_detects_prescriptive_statements() {
        assert!(is_action_intent(
            "This should be changed to use the new API"
        ));
        assert!(is_action_intent("We need to refactor this module"));
        assert!(is_action_intent("I should fix the null pointer bug"));
    }

    #[test]
    fn test_is_action_intent_ignores_pure_factual_claims() {
        assert!(!is_action_intent("The function returns a Result type"));
        assert!(!is_action_intent("tokio uses cooperative scheduling"));
    }

    #[test]
    fn test_is_action_intent_ignores_questions() {
        assert!(!is_action_intent("Should we refactor this?"));
        assert!(!is_action_intent("What do you want me to change?"));
    }
}
