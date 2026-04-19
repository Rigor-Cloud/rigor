use crate::claim::heuristic::extract_claims_from_text;
use crate::claim::transcript::TranscriptMessage;
/// ClaimExtractor trait and implementations.
///
/// Provides abstraction for different extraction strategies:
/// - HeuristicExtractor (v1): Rule-based extraction
/// - LLMExtractor (v2): LLM-based extraction (future)
use crate::claim::types::Claim;

/// Trait for extracting claims from transcript messages.
pub trait ClaimExtractor {
    /// Extract claims from a sequence of transcript messages.
    ///
    /// Implementations should:
    /// - Extract from latest assistant message
    /// - Use prior messages as context if needed
    /// - Generate unique claim IDs
    /// - Track source locations
    fn extract(&self, messages: &[TranscriptMessage]) -> Vec<Claim>;
}

/// Heuristic-based claim extractor (v1).
///
/// Uses pattern matching and NLP-light techniques:
/// - Sentence segmentation
/// - Hedge detection
/// - Assertion filtering
/// - Rule-based confidence scoring
pub struct HeuristicExtractor;

impl HeuristicExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HeuristicExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaimExtractor for HeuristicExtractor {
    fn extract(&self, messages: &[TranscriptMessage]) -> Vec<Claim> {
        // Find latest assistant message
        let latest_assistant = messages
            .iter()
            .enumerate()
            .rfind(|(_, msg)| msg.role == "assistant");

        match latest_assistant {
            Some((message_index, msg)) => extract_claims_from_text(&msg.text, message_index),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_message(role: &str, text: &str, message_index: usize) -> TranscriptMessage {
        TranscriptMessage {
            role: role.to_string(),
            text: text.to_string(),
            message_index,
        }
    }

    #[test]
    fn test_heuristic_extractor_basic() {
        let extractor = HeuristicExtractor::new();
        let messages = vec![
            make_message("user", "Tell me about async.", 0),
            make_message("assistant", "This library supports async.", 1),
        ];

        let claims = extractor.extract(&messages);
        assert_eq!(claims.len(), 1);
        assert!(claims[0].text.contains("async"));
    }

    #[test]
    fn test_extract_from_latest_assistant() {
        let extractor = HeuristicExtractor::new();
        let messages = vec![
            make_message("user", "First question.", 0),
            make_message("assistant", "First answer is here.", 1),
            make_message("user", "Second question.", 2),
            make_message("assistant", "Second answer is there.", 3),
        ];

        let claims = extractor.extract(&messages);

        // Should extract from latest assistant message only
        assert_eq!(claims.len(), 1);
        assert!(claims[0].text.contains("Second answer"));
    }

    #[test]
    fn test_no_assistant_messages() {
        let extractor = HeuristicExtractor::new();
        let messages = vec![
            make_message("user", "Question one.", 0),
            make_message("user", "Question two.", 1),
        ];

        let claims = extractor.extract(&messages);
        assert_eq!(claims.len(), 0);
    }

    #[test]
    fn test_filters_hedged_and_questions() {
        let extractor = HeuristicExtractor::new();
        let messages = vec![make_message(
            "assistant",
            "X supports Y. I think Z might work. Does W work?",
            0,
        )];

        let claims = extractor.extract(&messages);

        // Only "X supports Y." should be extracted
        assert_eq!(claims.len(), 1);
        assert!(claims[0].text.contains("X supports Y"));
    }

    #[test]
    fn test_message_index_tracking() {
        let extractor = HeuristicExtractor::new();
        let messages = vec![
            make_message("user", "First.", 0),
            make_message("assistant", "Second.", 1),
            make_message("user", "Third.", 2),
            make_message("assistant", "Fourth claim here.", 3),
        ];

        let claims = extractor.extract(&messages);

        assert_eq!(claims.len(), 1);
        // Latest assistant is at index 3
        assert_eq!(claims[0].source.as_ref().unwrap().message_index, 3);
    }
}
