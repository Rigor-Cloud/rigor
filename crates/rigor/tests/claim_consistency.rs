#![allow(
    clippy::await_holding_lock,
    clippy::single_match,
    clippy::bool_assert_comparison,
    clippy::doc_overindented_list_items
)]
//! Claim extraction consistency test.
//!
//! Verifies that `extract_claims_from_text()` produces IDENTICAL claims
//! regardless of whether the input text is:
//! (a) a direct string, or
//! (b) reassembled from SSE chunks (simulating the proxy path).
//!
//! This catches whitespace/encoding divergence between direct and
//! SSE-reassembled paths that could cause the proxy to evaluate different
//! claims than a direct caller would see.

use rigor::claim::heuristic::extract_claims_from_text;
use rigor_harness::sse::{
    anthropic_sse_chunks, extract_text_from_sse, openai_sse_chunks, SseFormat,
};

/// Fixed response text exercising multiple claim types: assertions, negations,
/// dependency claims, code references, hedged sentences, questions, and code
/// blocks. This covers the full `extract_claims_from_text` pipeline:
/// strip code blocks -> sentence segmentation -> assertion filter -> hedge
/// filter -> confidence + type classification.
const RESPONSE_TEXT: &str = "\
Rust uses ownership and borrowing for memory management. \
The tokio crate provides an async runtime for Rust. \
Python does not have a borrow checker. \
The PolicyEngine::new() function initializes the engine. \
Perhaps the performance could be improved. \
Does this support async? \
```rust\nfn main() {}\n```\n\
The regex library supports Unicode character classes. \
This module handles claim extraction from LLM responses.";

#[test]
fn direct_vs_anthropic_sse_reassembly_produces_identical_claims() {
    // Path A: direct extraction
    let direct_claims = extract_claims_from_text(RESPONSE_TEXT, 0);

    // Path B: chunk into Anthropic SSE, reassemble, then extract
    let chunks = anthropic_sse_chunks(RESPONSE_TEXT);
    let reassembled = extract_text_from_sse(&chunks, SseFormat::Anthropic);
    let sse_claims = extract_claims_from_text(&reassembled, 0);

    assert_eq!(
        direct_claims.len(),
        sse_claims.len(),
        "claim count mismatch: direct={}, SSE={}.\n  Direct texts: {:?}\n  SSE texts:    {:?}",
        direct_claims.len(),
        sse_claims.len(),
        direct_claims.iter().map(|c| &c.text).collect::<Vec<_>>(),
        sse_claims.iter().map(|c| &c.text).collect::<Vec<_>>(),
    );

    for (i, (d, s)) in direct_claims.iter().zip(sse_claims.iter()).enumerate() {
        assert_eq!(
            d.text, s.text,
            "claim {} text diverged:\n  direct: {:?}\n  sse:    {:?}",
            i, d.text, s.text,
        );
        assert_eq!(
            d.confidence, s.confidence,
            "claim {} confidence diverged: direct={}, sse={}",
            i, d.confidence, s.confidence,
        );
        assert_eq!(
            d.claim_type, s.claim_type,
            "claim {} type diverged: direct={:?}, sse={:?}",
            i, d.claim_type, s.claim_type,
        );
    }
}

#[test]
fn direct_vs_openai_sse_reassembly_produces_identical_claims() {
    // Path A: direct extraction
    let direct_claims = extract_claims_from_text(RESPONSE_TEXT, 0);

    // Path B: chunk into OpenAI SSE, reassemble, then extract
    let chunks = openai_sse_chunks(RESPONSE_TEXT);
    let reassembled = extract_text_from_sse(&chunks, SseFormat::OpenAI);
    let sse_claims = extract_claims_from_text(&reassembled, 0);

    assert_eq!(
        direct_claims.len(),
        sse_claims.len(),
        "claim count mismatch: direct={}, SSE={}.\n  Direct texts: {:?}\n  SSE texts:    {:?}",
        direct_claims.len(),
        sse_claims.len(),
        direct_claims.iter().map(|c| &c.text).collect::<Vec<_>>(),
        sse_claims.iter().map(|c| &c.text).collect::<Vec<_>>(),
    );

    for (i, (d, s)) in direct_claims.iter().zip(sse_claims.iter()).enumerate() {
        assert_eq!(
            d.text, s.text,
            "claim {} text diverged:\n  direct: {:?}\n  sse:    {:?}",
            i, d.text, s.text,
        );
        assert_eq!(
            d.confidence, s.confidence,
            "claim {} confidence diverged: direct={}, sse={}",
            i, d.confidence, s.confidence,
        );
        assert_eq!(
            d.claim_type, s.claim_type,
            "claim {} type diverged: direct={:?}, sse={:?}",
            i, d.claim_type, s.claim_type,
        );
    }
}

#[test]
fn reassembled_text_is_byte_identical_to_original() {
    // Verify the SSE round-trip itself preserves text exactly.
    // If this fails, the SSE chunking/reassembly is lossy.
    let anthropic_chunks = anthropic_sse_chunks(RESPONSE_TEXT);
    let anthropic_reassembled = extract_text_from_sse(&anthropic_chunks, SseFormat::Anthropic);
    assert_eq!(
        RESPONSE_TEXT, anthropic_reassembled,
        "Anthropic SSE round-trip is not byte-identical"
    );

    let openai_chunks = openai_sse_chunks(RESPONSE_TEXT);
    let openai_reassembled = extract_text_from_sse(&openai_chunks, SseFormat::OpenAI);
    assert_eq!(
        RESPONSE_TEXT, openai_reassembled,
        "OpenAI SSE round-trip is not byte-identical"
    );
}

#[test]
fn multiline_response_with_unicode_consistency() {
    // Stress test: multi-paragraph response with Unicode characters, newlines,
    // and special punctuation that might be mangled by JSON escaping in SSE.
    let text = "The Dijkstra algorithm finds shortest paths in O(V log V) time. \
The crate supports UTF-8 natively \u{2014} including CJK characters. \
Rust\u{2019}s type system prevents data races at compile time.";

    let direct = extract_claims_from_text(text, 0);

    let chunks = anthropic_sse_chunks(text);
    let reassembled = extract_text_from_sse(&chunks, SseFormat::Anthropic);
    let sse = extract_claims_from_text(&reassembled, 0);

    assert_eq!(
        direct.len(),
        sse.len(),
        "Unicode text: claim count mismatch"
    );
    for (i, (d, s)) in direct.iter().zip(sse.iter()).enumerate() {
        assert_eq!(d.text, s.text, "Unicode claim {} text diverged", i);
    }
}
