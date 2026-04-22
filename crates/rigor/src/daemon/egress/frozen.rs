//! Frozen-prefix invariant for the egress pipeline (§5.6 "0F").
//!
//! Once `set_frozen_prefix` seals the first `message_count` messages of a
//! request, the post-chain verifier MUST see an identical byte-checksum over
//! `messages[0..message_count]`. Any request filter that needs to edit the
//! frozen range MUST explicitly call `set_frozen_prefix` again with the new
//! baseline.
//!
//! Backward compat: if no `FrozenPrefix` is present in `ConversationCtx::scratch`,
//! `verify_frozen_prefix` is a no-op (returns `Ok(())`).

use serde_json::Value as Json;
use std::hash::Hasher;
use twox_hash::XxHash64;

use super::chain::FilterError;
use super::ctx::ConversationCtx;

/// Sealed invariant stored in `ConversationCtx::scratch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrozenPrefix {
    pub message_count: usize,
    pub byte_checksum: u64,
}

/// xxhash64 of canonical JSON bytes for each message, concatenated in order.
///
/// Deterministic across runs: `serde_json::to_vec` is stable for a given
/// `Value` (maps use their internal BTreeMap/preserve-order iteration, and
/// values are not reordered within this function). A `0u8` separator is
/// written between messages so `["ab", "c"]` and `["a", "bc"]` do not
/// hash-collide.
///
/// Non-cryptographic: this is a speed-optimised checksum for detecting
/// accidental mutation of the frozen prefix. It MUST NOT be used anywhere a
/// cryptographic hash is required — content-addressing uses `sha2::Sha256`.
pub fn compute_checksum(messages: &[Json]) -> u64 {
    let mut hasher = XxHash64::with_seed(0);
    for msg in messages {
        match serde_json::to_vec(msg) {
            Ok(bytes) => hasher.write(&bytes),
            Err(_) => hasher.write(b"<unserializable>"),
        }
        // Separator so "ab" || "c" != "a" || "bc".
        hasher.write(&[0u8]);
    }
    hasher.finish()
}

/// Seal the first `freeze_count` messages into the context scratch.
///
/// `freeze_count` is clamped to `messages.len()` so callers cannot
/// accidentally seal a range larger than the slice they provide.
/// Overwrites any previous `FrozenPrefix`.
pub fn set_frozen_prefix(ctx: &mut ConversationCtx, messages: &[Json], freeze_count: usize) {
    let effective = freeze_count.min(messages.len());
    let checksum = compute_checksum(&messages[..effective]);
    ctx.scratch_set(FrozenPrefix {
        message_count: effective,
        byte_checksum: checksum,
    });
}

/// Post-chain verifier. Called from `FilterChain::apply_request` after all
/// request filters have run.
///
/// Returns:
/// - `Ok(())` if no `FrozenPrefix` is sealed (backward compat no-op).
/// - `Ok(())` if the checksum over `messages[0..message_count]` matches.
/// - `Err(FilterError::Internal)` if `messages` is shorter than the sealed
///   count, or if the recomputed checksum diverges from the sealed one.
pub fn verify_frozen_prefix(ctx: &ConversationCtx, messages: &[Json]) -> Result<(), FilterError> {
    let Some(frozen) = ctx.scratch_get::<FrozenPrefix>() else {
        return Ok(());
    };
    if messages.len() < frozen.message_count {
        return Err(FilterError::Internal {
            filter: "frozen_prefix".into(),
            reason: format!(
                "messages shorter than frozen count ({} < {})",
                messages.len(),
                frozen.message_count
            ),
        });
    }
    let actual = compute_checksum(&messages[..frozen.message_count]);
    if actual == frozen.byte_checksum {
        Ok(())
    } else {
        Err(FilterError::Internal {
            filter: "frozen_prefix".into(),
            reason: format!(
                "frozen-prefix checksum mismatch: expected {:#x}, got {:#x} (first {} messages)",
                frozen.byte_checksum, actual, frozen.message_count
            ),
        })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msgs() -> Vec<Json> {
        vec![
            json!({"role": "user", "content": "hello"}),
            json!({"role": "assistant", "content": "hi there"}),
            json!({"role": "user", "content": "how are you"}),
        ]
    }

    #[test]
    fn compute_checksum_is_deterministic() {
        let a = compute_checksum(&msgs());
        let b = compute_checksum(&msgs());
        assert_eq!(a, b, "checksum must be deterministic across calls");
    }

    #[test]
    fn compute_checksum_differs_on_content_change() {
        let mut a = msgs();
        let b = msgs();
        a[0]["content"] = json!("different");
        assert_ne!(compute_checksum(&a), compute_checksum(&b));
    }

    #[test]
    fn set_then_verify_ok_unchanged() {
        let mut ctx = ConversationCtx::new_anonymous();
        let messages = msgs();
        set_frozen_prefix(&mut ctx, &messages, 1);
        assert!(verify_frozen_prefix(&ctx, &messages).is_ok());
    }

    #[test]
    fn verify_err_when_frozen_range_mutated() {
        let mut ctx = ConversationCtx::new_anonymous();
        let messages = msgs();
        set_frozen_prefix(&mut ctx, &messages, 2);

        let mut tampered = messages.clone();
        tampered[0]["content"] = json!("MUTATED");
        let err =
            verify_frozen_prefix(&ctx, &tampered).expect_err("tampered frozen range must fail");
        match err {
            FilterError::Internal { filter, reason } => {
                assert_eq!(filter, "frozen_prefix");
                assert!(reason.contains("checksum mismatch"), "got {reason}");
            }
            other => panic!("unexpected error variant: {:?}", other),
        }
    }

    #[test]
    fn verify_ok_when_only_tail_mutated() {
        let mut ctx = ConversationCtx::new_anonymous();
        let messages = msgs();
        set_frozen_prefix(&mut ctx, &messages, 1);

        let mut modified = messages.clone();
        modified[2]["content"] = json!("changed-but-outside-freeze");
        modified.push(json!({"role": "assistant", "content": "new turn"}));
        assert!(verify_frozen_prefix(&ctx, &modified).is_ok());
    }

    #[test]
    fn verify_ok_when_no_frozen_prefix_in_scratch() {
        // Backward compat: a fresh ctx has no FrozenPrefix -> verify is no-op.
        let ctx = ConversationCtx::new_anonymous();
        assert!(verify_frozen_prefix(&ctx, &[]).is_ok());
        assert!(verify_frozen_prefix(&ctx, &msgs()).is_ok());
    }

    #[test]
    fn verify_err_when_message_count_out_of_bounds() {
        let mut ctx = ConversationCtx::new_anonymous();
        let full = msgs();
        set_frozen_prefix(&mut ctx, &full, 3);

        let truncated = vec![full[0].clone()];
        let err = verify_frozen_prefix(&ctx, &truncated)
            .expect_err("truncated messages shorter than frozen count must fail");
        assert!(matches!(err, FilterError::Internal { .. }));
    }

    #[test]
    fn set_frozen_prefix_clamps_to_messages_len() {
        // freeze_count larger than messages.len() clamps to messages.len()
        let mut ctx = ConversationCtx::new_anonymous();
        let m = msgs();
        set_frozen_prefix(&mut ctx, &m, 99);
        let fp = ctx
            .scratch_get::<FrozenPrefix>()
            .expect("FrozenPrefix must be set");
        assert_eq!(fp.message_count, m.len());
        assert!(verify_frozen_prefix(&ctx, &m).is_ok());
    }
}
