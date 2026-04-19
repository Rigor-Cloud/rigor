//! Action gate state management.
use std::time::{Duration, Instant};

use super::{ActionGateEntry, GateDecision, GateType, SharedState};
use super::ws::{DaemonEvent, EventSender};

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
        st.action_gates.insert(gate_id.clone(), ActionGateEntry {
            gate_type: GateType::RealTime,
            decision_tx: Some(tx),
            action_text: action_text.clone(),
            user_message: user_message.clone(),
            session_id,
            created_at: Instant::now(),
        });
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
        let snaps = st.gate_snapshots.get(&session_id).cloned().unwrap_or_default();
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
        st.action_gates.insert(gate_id.clone(), ActionGateEntry {
            gate_type: GateType::Retroactive,
            decision_tx: None,
            action_text: action_text.clone(),
            user_message: user_message.clone(),
            session_id,
            created_at: Instant::now(),
        });
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
        let entry = st.action_gates.get_mut(gate_id)
            .ok_or_else(|| format!("Gate {} not found", gate_id))?;
        let session = entry.session_id.clone();
        if let Some(tx) = entry.decision_tx.take() {
            let _ = tx.send(approved);
        }
        session
    };

    {
        let mut st = state.lock().unwrap();
        st.gate_decisions.insert(session_id, GateDecision {
            approved,
            gate_id: gate_id.to_string(),
            decided_at: Instant::now(),
        });
    }
    Ok(())
}

/// Clean up gates older than GATE_TIMEOUT_SECS — they auto-reject.
pub fn cleanup_expired_gates(state: &SharedState) -> Vec<String> {
    let mut expired = Vec::new();
    let mut st = state.lock().unwrap();
    let cutoff = Duration::from_secs(GATE_TIMEOUT_SECS);
    let now = Instant::now();

    let to_remove: Vec<String> = st.action_gates.iter()
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
