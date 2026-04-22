//! Per-session cost estimation for LLM API usage.
//!
//! Provides approximate USD pricing for common models based on published
//! per-token rates. These are estimates — actual billing may differ due to
//! batching, caching, or provider-specific discounts.

/// Estimate the cost in USD for a given model and token counts.
///
/// Pricing is approximate and based on published rates as of mid-2025.
/// Unknown models fall back to a conservative estimate.
pub fn estimate_cost(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    let model_lower = model.to_lowercase();

    // (input_price_per_million, output_price_per_million)
    let (input_ppm, output_ppm) = if model_lower.contains("claude-opus")
        || model_lower.contains("claude-4-opus")
        || model_lower.contains("opus-4")
    {
        (15.0, 75.0)
    } else if model_lower.contains("claude-sonnet")
        || model_lower.contains("claude-4-sonnet")
        || model_lower.contains("sonnet-4")
        || model_lower.contains("claude-3-5-sonnet")
        || model_lower.contains("claude-3.5-sonnet")
    {
        (3.0, 15.0)
    } else if model_lower.contains("claude-3-5-haiku")
        || model_lower.contains("claude-3.5-haiku")
        || model_lower.contains("claude-haiku")
    {
        (0.80, 4.0)
    } else if model_lower.contains("gpt-4o-mini") {
        (0.15, 0.60)
    } else if model_lower.contains("gpt-4o") {
        (2.50, 10.0)
    } else if model_lower.contains("gpt-4-turbo") || model_lower.contains("gpt-4-1") {
        (10.0, 30.0)
    } else if model_lower.contains("gpt-4") {
        (30.0, 60.0)
    } else if model_lower.contains("gpt-5") || model_lower.contains("gpt-5-nano") {
        (2.0, 8.0)
    } else if model_lower.contains("o3-mini") || model_lower.contains("o1-mini") {
        (3.0, 12.0)
    } else if model_lower.contains("o3") || model_lower.contains("o1") {
        (15.0, 60.0)
    } else if model_lower.contains("gemini-2") || model_lower.contains("gemini-1.5-pro") {
        (3.50, 10.50)
    } else if model_lower.contains("gemini-1.5-flash") || model_lower.contains("gemini-flash") {
        (0.075, 0.30)
    } else if model_lower.contains("deepseek") {
        (0.27, 1.10)
    } else if model_lower.contains("mistral-large") {
        (2.0, 6.0)
    } else if model_lower.contains("llama") {
        (0.20, 0.20)
    } else {
        // Conservative fallback: assume mid-tier pricing
        (3.0, 15.0)
    };

    let input_cost = (input_tokens as f64 / 1_000_000.0) * input_ppm;
    let output_cost = (output_tokens as f64 / 1_000_000.0) * output_ppm;

    input_cost + output_cost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_opus_pricing() {
        let cost = estimate_cost("claude-opus-4-7", 1_000_000, 1_000_000);
        // $15 input + $75 output = $90
        assert!((cost - 90.0).abs() < 0.01);
    }

    #[test]
    fn test_claude_sonnet_pricing() {
        let cost = estimate_cost("claude-sonnet-4-5", 1_000_000, 1_000_000);
        // $3 input + $15 output = $18
        assert!((cost - 18.0).abs() < 0.01);
    }

    #[test]
    fn test_gpt4o_pricing() {
        let cost = estimate_cost("gpt-4o", 1_000_000, 1_000_000);
        // $2.50 input + $10 output = $12.50
        assert!((cost - 12.5).abs() < 0.01);
    }

    #[test]
    fn test_unknown_model_fallback() {
        let cost = estimate_cost("some-unknown-model", 1_000_000, 1_000_000);
        // Fallback: $3 + $15 = $18
        assert!((cost - 18.0).abs() < 0.01);
    }

    #[test]
    fn test_zero_tokens() {
        let cost = estimate_cost("claude-opus-4-7", 0, 0);
        assert!((cost - 0.0).abs() < 0.001);
    }
}
