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

use super::chain::FilterError;
use super::ctx::ConversationCtx;

/// Sealed invariant stored in `ConversationCtx::scratch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrozenPrefix {
    pub message_count: usize,
    pub byte_checksum: u64,
}

/// xxhash64 of canonical JSON bytes for each message, concatenated in order.
pub fn compute_checksum(_messages: &[Json]) -> u64 {
    todo!("compute_checksum not yet implemented (RED phase)")
}

/// Seal the first `freeze_count` messages into the context scratch.
pub fn set_frozen_prefix(
    _ctx: &mut ConversationCtx,
    _messages: &[Json],
    _freeze_count: usize,
) {
    todo!("set_frozen_prefix not yet implemented (RED phase)")
}

/// Post-chain verifier. Called from `FilterChain::apply_request` after all
/// request filters have run.
pub fn verify_frozen_prefix(
    _ctx: &ConversationCtx,
    _messages: &[Json],
) -> Result<(), FilterError> {
    todo!("verify_frozen_prefix not yet implemented (RED phase)")
}

// ===========================================================================
// Tests (RED phase — these MUST fail before implementation)
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
        let err = verify_frozen_prefix(&ctx, &tampered)
            .expect_err("tampered frozen range must fail");
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
