//! Action gate state management.
use std::time::{Duration, Instant};

use super::ws::{DaemonEvent, EventSender};
use super::{ActionGateEntry, GateDecision, GateType, SharedState};

pub const GATE_TIMEOUT_SECS: u64 = 60;

/// Create a realtime gate, return oneshot receiver for stream to await.
pub fn create_realtime_gate(
    state: &SharedState,
    request_id: String,
    gate_id: String,
    action_text: String,
    user_message: String,
    session_id: String,
    reason: String,
    event_tx: &EventSender,
) -> tokio::sync::oneshot::Receiver<bool> {
    let (tx, rx) = tokio::sync::oneshot::channel();

    {
        let mut st = state.lock().unwrap();
        st.action_gates.insert(
            gate_id.clone(),
            ActionGateEntry {
                gate_type: GateType::RealTime,
                decision_tx: Some(tx),
                action_text: action_text.clone(),
                user_message: user_message.clone(),
                session_id,
                created_at: Instant::now(),
            },
        );
    }

    let _ = event_tx.send(DaemonEvent::ActionGate {
        request_id,
        gate_id,
        gate_type: "realtime".to_string(),
        action_text,
        user_message,
        reason,
        revertable_paths: Vec::new(),
        non_revertable: Vec::new(),
    });

    rx
}

/// Register a retroactive gate — next request from this session will block.
pub fn register_retroactive_gate(
    state: &SharedState,
    request_id: String,
    gate_id: String,
    action_text: String,
    user_message: String,
    session_id: String,
    reason: String,
    event_tx: &EventSender,
) {
    let (revertable, non_revertable) = {
        let st = state.lock().unwrap();
        let snaps = st
            .gate_snapshots
            .get(&session_id)
            .cloned()
            .unwrap_or_default();
        let mut rev = Vec::new();
        let mut non_rev = Vec::new();
        for s in &snaps {
            if s.tool_name == "Edit" || s.tool_name == "Write" {
                rev.extend(s.affected_paths.clone());
            } else {
                non_rev.push(format!("{}: {}", s.tool_name, s.affected_paths.join(", ")));
            }
        }
        (rev, non_rev)
    };

    {
        let mut st = state.lock().unwrap();
        st.action_gates.insert(
            gate_id.clone(),
            ActionGateEntry {
                gate_type: GateType::Retroactive,
                decision_tx: None,
                action_text: action_text.clone(),
                user_message: user_message.clone(),
                session_id,
                created_at: Instant::now(),
            },
        );
    }

    let _ = event_tx.send(DaemonEvent::ActionGate {
        request_id,
        gate_id,
        gate_type: "retroactive".to_string(),
        action_text,
        user_message,
        reason,
        revertable_paths: revertable,
        non_revertable,
    });
}

/// Apply a decision to a gate.
pub fn apply_decision(state: &SharedState, gate_id: &str, approved: bool) -> Result<(), String> {
    let session_id = {
        let mut st = state.lock().unwrap();
        let entry = st
            .action_gates
            .get_mut(gate_id)
            .ok_or_else(|| format!("Gate {} not found", gate_id))?;
        let session = entry.session_id.clone();
        if let Some(tx) = entry.decision_tx.take() {
            let _ = tx.send(approved);
        }
        session
    };

    {
        let mut st = state.lock().unwrap();
        st.gate_decisions.insert(
            session_id,
            GateDecision {
                approved,
                gate_id: gate_id.to_string(),
                decided_at: Instant::now(),
            },
        );
    }
    Ok(())
}

/// Clean up gates older than GATE_TIMEOUT_SECS — they auto-reject.
pub fn cleanup_expired_gates(state: &SharedState) -> Vec<String> {
    let mut expired = Vec::new();
    let mut st = state.lock().unwrap();
    let cutoff = Duration::from_secs(GATE_TIMEOUT_SECS);
    let now = Instant::now();

    let to_remove: Vec<String> = st
        .action_gates
        .iter()
        .filter(|(_, e)| now.duration_since(e.created_at) > cutoff)
        .map(|(id, _)| id.clone())
        .collect();

    for id in &to_remove {
        if let Some(mut entry) = st.action_gates.remove(id) {
            if let Some(tx) = entry.decision_tx.take() {
                let _ = tx.send(false);
            }
            expired.push(id.clone());
        }
    }
    expired
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    #![allow(
        clippy::await_holding_lock,
        clippy::bool_assert_comparison,
        clippy::single_match
    )]
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Helper: save RIGOR_HOME, set to tempdir, run closure, restore.
    /// Uses the crate-wide RIGOR_HOME_TEST_LOCK to serialize across all
    /// test modules that mutate this env var.
    fn with_temp_rigor_home<F: FnOnce()>(f: F) {
        let _guard = crate::paths::RIGOR_HOME_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let original = std::env::var("RIGOR_HOME").ok();
        let tmp = tempfile::TempDir::new().unwrap();
        let rigor_dir = tmp.path().join(".rigor");
        unsafe { std::env::set_var("RIGOR_HOME", &rigor_dir) };

        f();

        match original {
            Some(v) => unsafe { std::env::set_var("RIGOR_HOME", v) },
            None => unsafe { std::env::remove_var("RIGOR_HOME") },
        }
    }

    /// Construct a minimal SharedState + EventSender for gate tests.
    fn make_test_state() -> (SharedState, EventSender) {
        let (event_tx, _rx) = super::super::ws::create_event_channel();
        let state = super::super::DaemonState::empty(event_tx.clone()).unwrap();
        (Arc::new(Mutex::new(state)), event_tx)
    }

    #[test]
    fn test_create_realtime_gate_returns_receiver() {
        with_temp_rigor_home(|| {
            let (state, event_tx) = make_test_state();
            let mut rx = create_realtime_gate(
                &state,
                "req-1".to_string(),
                "gate-1".to_string(),
                "action text".to_string(),
                "user message".to_string(),
                "sess-1".to_string(),
                "reason".to_string(),
                &event_tx,
            );

            // The receiver should not be closed yet (sender is held in the gate entry).
            assert!(
                rx.try_recv().is_err(),
                "receiver should not have a value yet (sender still held)"
            );

            // Verify the gate is stored in state.
            let st = state.lock().unwrap();
            assert!(
                st.action_gates.contains_key("gate-1"),
                "gate-1 should be stored in action_gates"
            );
        });
    }

    #[test]
    fn test_apply_decision_approved_sends_true() {
        with_temp_rigor_home(|| {
            let (state, event_tx) = make_test_state();
            let mut rx = create_realtime_gate(
                &state,
                "req-2".to_string(),
                "gate-2".to_string(),
                "action".to_string(),
                "message".to_string(),
                "sess-2".to_string(),
                "reason".to_string(),
                &event_tx,
            );

            apply_decision(&state, "gate-2", true).unwrap();
            assert_eq!(
                rx.try_recv().unwrap(),
                true,
                "approved decision should send true on oneshot channel"
            );
        });
    }

    #[test]
    fn test_apply_decision_rejected_sends_false() {
        with_temp_rigor_home(|| {
            let (state, event_tx) = make_test_state();
            let mut rx = create_realtime_gate(
                &state,
                "req-3".to_string(),
                "gate-3".to_string(),
                "action".to_string(),
                "message".to_string(),
                "sess-3".to_string(),
                "reason".to_string(),
                &event_tx,
            );

            apply_decision(&state, "gate-3", false).unwrap();
            assert_eq!(
                rx.try_recv().unwrap(),
                false,
                "rejected decision should send false on oneshot channel"
            );
        });
    }

    #[test]
    fn test_apply_decision_nonexistent_gate_returns_err() {
        with_temp_rigor_home(|| {
            let (state, _event_tx) = make_test_state();
            let result = apply_decision(&state, "nonexistent-gate", true);
            assert!(
                result.is_err(),
                "apply_decision on nonexistent gate should error"
            );
            assert!(
                result.unwrap_err().contains("not found"),
                "error message should mention 'not found'"
            );
        });
    }

    #[test]
    fn test_cleanup_expired_gates_auto_rejects() {
        with_temp_rigor_home(|| {
            let (state, _event_tx) = make_test_state();

            // Manually insert an expired gate (created_at 61 seconds ago).
            let (tx, mut rx) = tokio::sync::oneshot::channel();
            {
                let mut st = state.lock().unwrap();
                st.action_gates.insert(
                    "expired-gate".to_string(),
                    super::super::ActionGateEntry {
                        gate_type: super::super::GateType::RealTime,
                        decision_tx: Some(tx),
                        action_text: "expired action".to_string(),
                        user_message: "expired msg".to_string(),
                        session_id: "sess-expired".to_string(),
                        created_at: Instant::now() - Duration::from_secs(61),
                    },
                );
            }

            let expired = cleanup_expired_gates(&state);
            assert!(
                expired.contains(&"expired-gate".to_string()),
                "cleanup should return the expired gate id"
            );

            // Gate should be removed from state.
            let st = state.lock().unwrap();
            assert!(
                !st.action_gates.contains_key("expired-gate"),
                "expired gate should be removed from action_gates"
            );
            drop(st);

            // The oneshot receiver should get false (auto-rejected).
            assert_eq!(
                rx.try_recv().unwrap(),
                false,
                "expired gate should be auto-rejected (false)"
            );
        });
    }

    #[test]
    fn test_cleanup_does_not_remove_fresh_gates() {
        with_temp_rigor_home(|| {
            let (state, event_tx) = make_test_state();
            let _rx = create_realtime_gate(
                &state,
                "req-fresh".to_string(),
                "fresh-gate".to_string(),
                "action".to_string(),
                "message".to_string(),
                "sess-fresh".to_string(),
                "reason".to_string(),
                &event_tx,
            );

            let expired = cleanup_expired_gates(&state);
            assert!(
                expired.is_empty(),
                "cleanup should not remove freshly created gates"
            );

            let st = state.lock().unwrap();
            assert!(
                st.action_gates.contains_key("fresh-gate"),
                "fresh gate should still exist in action_gates"
            );
        });
    }
}
