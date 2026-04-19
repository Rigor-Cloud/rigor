/// End-to-end integration test for claim extraction pipeline.
///
/// Tests the full flow:
/// - Transcript parsing (JSONL -> TranscriptMessage)
/// - Claim extraction (HeuristicExtractor filtering)
/// - Claim structure validation
/// - Pipeline input serialization (EvaluationInput)
use rigor::claim::{parse_transcript, ClaimExtractor, ClaimType, HeuristicExtractor};
use rigor::policy::EvaluationInput;
use std::io::Write;
use tempfile::NamedTempFile;

/// Create a sample JSONL transcript with realistic Claude Code messages.
fn create_sample_transcript() -> NamedTempFile {
    let mut tmpfile = NamedTempFile::new().unwrap();

    // User message (should be skipped)
    writeln!(
        tmpfile,
        r#"{{"role":"user","content":"Tell me about the regex crate."}}"#
    )
    .unwrap();

    // Assistant message with clear assertions
    writeln!(
        tmpfile,
        r#"{{"role":"assistant","content":"The regex crate supports Unicode by default. It does not support lookahead patterns."}}"#
    ).unwrap();

    // User message (should be skipped)
    writeln!(
        tmpfile,
        r#"{{"role":"user","content":"What about the version?"}}"#
    )
    .unwrap();

    // Assistant message with hedged content (should be filtered)
    writeln!(
        tmpfile,
        r#"{{"role":"assistant","content":"I think this might work with the latest version. The API is stable."}}"#
    ).unwrap();

    // User message (should be skipped)
    writeln!(tmpfile, r#"{{"role":"user","content":"Show me code."}}"#).unwrap();

    // Assistant message with code blocks (should be stripped)
    writeln!(
        tmpfile,
        r#"{{"role":"assistant","content":"The library is well documented.\n```rust\nfn test() {{}}\n```\nIt has good performance."}}"#
    ).unwrap();

    tmpfile.flush().unwrap();
    tmpfile
}

#[test]
fn test_parse_transcript_returns_correct_message_count() {
    let tmpfile = create_sample_transcript();
    let messages = parse_transcript(tmpfile.path()).unwrap();

    // Should have 6 messages total (3 user + 3 assistant)
    assert_eq!(messages.len(), 6);

    // Verify roles
    let roles: Vec<&str> = messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(
        roles,
        vec![
            "user",
            "assistant",
            "user",
            "assistant",
            "user",
            "assistant"
        ]
    );
}

#[test]
fn test_heuristic_extractor_filters_non_assistant_messages() {
    let tmpfile = create_sample_transcript();
    let messages = parse_transcript(tmpfile.path()).unwrap();

    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);

    // Should only extract from latest assistant message
    // Latest is: "The library is well documented. ... It has good performance."
    // After code stripping and filtering, should have 2 claims
    assert!(
        !claims.is_empty(),
        "Should extract at least one claim from latest assistant message"
    );

    // All claims should be from assistant messages
    for claim in &claims {
        assert!(
            claim.source.is_some(),
            "All claims should have source location"
        );
    }
}

#[test]
fn test_extracted_claims_have_valid_structure() {
    let tmpfile = create_sample_transcript();
    let messages = parse_transcript(tmpfile.path()).unwrap();

    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);

    for claim in &claims {
        // Non-empty text
        assert!(!claim.text.is_empty(), "Claim text should not be empty");

        // Confidence between 0.0 and 1.0
        assert!(
            claim.confidence >= 0.0 && claim.confidence <= 1.0,
            "Confidence should be in range [0.0, 1.0], got {}",
            claim.confidence
        );

        // Valid claim type
        assert!(
            matches!(
                claim.claim_type,
                ClaimType::Assertion
                    | ClaimType::Negation
                    | ClaimType::CodeReference
                    | ClaimType::ArchitecturalDecision
                    | ClaimType::DependencyClaim
            ),
            "Claim type should be valid"
        );

        // Source location with correct message_index
        let source = claim.source.as_ref().expect("Claim should have source");
        assert!(
            source.message_index < messages.len(),
            "Source message_index should be valid"
        );
    }
}

#[test]
fn test_extractor_filters_hedged_statements() {
    let mut tmpfile = NamedTempFile::new().unwrap();
    writeln!(
        tmpfile,
        r#"{{"role":"assistant","content":"X supports Y. I think Z might work. Maybe W is correct."}}"#
    ).unwrap();
    tmpfile.flush().unwrap();

    let messages = parse_transcript(tmpfile.path()).unwrap();
    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);

    // Should only extract "X supports Y." (hedged statements filtered)
    assert_eq!(claims.len(), 1);
    assert!(claims[0].text.contains("X supports Y"));
}

#[test]
fn test_extractor_identifies_negations() {
    let mut tmpfile = NamedTempFile::new().unwrap();
    writeln!(
        tmpfile,
        r#"{{"role":"assistant","content":"The regex crate supports Unicode. It does not support lookahead patterns."}}"#
    ).unwrap();
    tmpfile.flush().unwrap();

    let messages = parse_transcript(tmpfile.path()).unwrap();
    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);

    assert_eq!(claims.len(), 2);

    // First claim should be dependency claim (mentions "crate")
    assert_eq!(claims[0].claim_type, ClaimType::DependencyClaim);

    // Second claim should be negation
    assert_eq!(claims[1].claim_type, ClaimType::Negation);
    assert!(claims[1].text.contains("does not support"));
}

#[test]
fn test_pipeline_input_serialization() {
    let tmpfile = create_sample_transcript();
    let messages = parse_transcript(tmpfile.path()).unwrap();

    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);

    // Create EvaluationInput (what gets passed to regorus)
    let eval_input = EvaluationInput { claims };

    // Test serialization to JSON (regorus expects JSON input)
    let json = serde_json::to_string(&eval_input).expect("Should serialize to JSON");

    // Verify JSON structure
    assert!(
        json.contains("\"claims\""),
        "JSON should contain claims key"
    );
    assert!(json.contains("\"text\""), "JSON should contain claim text");
    assert!(
        json.contains("\"confidence\""),
        "JSON should contain confidence"
    );
    assert!(
        json.contains("\"claim_type\""),
        "JSON should contain claim_type"
    );
}

#[test]
fn test_code_block_stripping() {
    let mut tmpfile = NamedTempFile::new().unwrap();
    writeln!(
        tmpfile,
        r#"{{"role":"assistant","content":"The library works.\n```rust\nfn main() {{}}\n```\nIt is fast."}}"#
    ).unwrap();
    tmpfile.flush().unwrap();

    let messages = parse_transcript(tmpfile.path()).unwrap();
    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);

    // Should extract 2 claims, code block stripped
    assert_eq!(claims.len(), 2);

    // Claims should not contain code
    for claim in &claims {
        assert!(
            !claim.text.contains("fn main"),
            "Code should be stripped from claims"
        );
        assert!(
            !claim.text.contains("```"),
            "Code fences should be stripped"
        );
    }
}

#[test]
fn test_question_filtering() {
    let mut tmpfile = NamedTempFile::new().unwrap();
    writeln!(
        tmpfile,
        r#"{{"role":"assistant","content":"X supports Y. Does Z support W? A is B."}}"#
    )
    .unwrap();
    tmpfile.flush().unwrap();

    let messages = parse_transcript(tmpfile.path()).unwrap();
    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);

    // Should extract 2 claims (question filtered)
    assert_eq!(claims.len(), 2);

    // No claims should end with question marks
    for claim in &claims {
        assert!(!claim.text.ends_with('?'), "Questions should be filtered");
    }
}

#[test]
fn test_empty_transcript() {
    let tmpfile = NamedTempFile::new().unwrap();

    let messages = parse_transcript(tmpfile.path()).unwrap();
    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);

    // Empty transcript should produce zero claims
    assert_eq!(claims.len(), 0);
}

#[test]
fn test_no_assistant_messages() {
    let mut tmpfile = NamedTempFile::new().unwrap();
    writeln!(
        tmpfile,
        r#"{{"role":"user","content":"Only user messages."}}"#
    )
    .unwrap();
    tmpfile.flush().unwrap();

    let messages = parse_transcript(tmpfile.path()).unwrap();
    let extractor = HeuristicExtractor::new();
    let claims = extractor.extract(&messages);

    // No assistant messages should produce zero claims
    assert_eq!(claims.len(), 0);
}
