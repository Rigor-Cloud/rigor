/// Hedge detection for filtering uncertain statements.
///
/// Hedged statements (e.g., "I think X is true") are filtered out
/// to avoid treating uncertain claims as facts.
use once_cell::sync::Lazy;
use regex::Regex;

/// Regex pattern matching hedge words.
static HEDGE_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(I think|probably|might|maybe|perhaps|possibly|likely|seems?|appears?|could be|I believe|I assume|I guess|not sure)\b"
    ).expect("Valid hedge pattern regex")
});

/// Returns true if the sentence contains hedge words indicating uncertainty.
pub fn is_hedged(sentence: &str) -> bool {
    HEDGE_PATTERN.is_match(sentence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hedge_i_think() {
        assert!(is_hedged("I think this is correct"));
    }

    #[test]
    fn test_no_hedge_definitive() {
        assert!(!is_hedged("This library supports async"));
    }

    #[test]
    fn test_hedge_probably() {
        assert!(is_hedged("It probably works"));
    }

    #[test]
    fn test_hedge_might() {
        assert!(is_hedged("The function might fail"));
    }

    #[test]
    fn test_hedge_perhaps() {
        assert!(is_hedged("perhaps we should use this"));
    }

    #[test]
    fn test_empty_string() {
        assert!(!is_hedged(""));
    }

    #[test]
    fn test_case_insensitive() {
        assert!(is_hedged("I THINK this works"));
        assert!(is_hedged("Maybe this is right"));
    }
}
