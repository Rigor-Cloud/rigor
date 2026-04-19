use once_cell::sync::Lazy;
/// Rule-based confidence scoring for claims.
///
/// Assigns confidence scores based on linguistic patterns:
/// - Definitive markers ("is", "does", "are", "has") → 0.9
/// - Negation markers ("not", "doesn't", "cannot") → 0.8
/// - Default → 0.7
use regex::Regex;

static DEFINITIVE_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(is|does|are|has|will)\b").expect("Valid definitive pattern"));

static NEGATION_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(not|doesn't|don't|cannot|can't|never|won't)\b")
        .expect("Valid negation pattern")
});

/// Assign confidence score to a sentence based on linguistic patterns.
///
/// Returns a value between 0.0 and 1.0 where:
/// - 0.9 indicates definitive statements
/// - 0.8 indicates negations
/// - 0.7 is the default for general assertions
pub fn assign_confidence(sentence: &str) -> f64 {
    // Check for negation first (more specific)
    if NEGATION_PATTERN.is_match(sentence) {
        return 0.8;
    }

    // Check for definitive markers
    if DEFINITIVE_PATTERN.is_match(sentence) {
        return 0.9;
    }

    // Default confidence
    0.7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_definitive_is() {
        assert_eq!(assign_confidence("X is Y"), 0.9);
    }

    #[test]
    fn test_definitive_does() {
        assert_eq!(assign_confidence("X does Y"), 0.9);
    }

    #[test]
    fn test_negation_not() {
        assert_eq!(assign_confidence("X does not support Y"), 0.8);
    }

    #[test]
    fn test_negation_doesnt() {
        assert_eq!(assign_confidence("X doesn't work"), 0.8);
    }

    #[test]
    fn test_default() {
        assert_eq!(assign_confidence("X works with Y"), 0.7);
    }

    #[test]
    fn test_negation_priority() {
        // Negation should take priority over definitive
        assert_eq!(assign_confidence("This is not supported"), 0.8);
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(assign_confidence("X IS Y"), 0.9);
        assert_eq!(assign_confidence("X DOESN'T work"), 0.8);
    }
}
