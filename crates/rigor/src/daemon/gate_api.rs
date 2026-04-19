//! HTTP API endpoints for action gates.
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::SharedState;

#[derive(Deserialize)]
pub struct RegisterSnapshotBody {
    pub session_id: String,
    pub snapshot_id: String,
    pub tool_name: String,
    pub affected_paths: Vec<String>,
}

pub async fn register_snapshot(
    State(state): State<SharedState>,
    Json(body): Json<RegisterSnapshotBody>,
) -> impl IntoResponse {
    let mut st = state.lock().unwrap();
    let entry = super::SnapshotEntry {
        snapshot_id: body.snapshot_id,
        affected_paths: body.affected_paths,
        tool_name: body.tool_name,
        created_at: std::time::Instant::now(),
    };
    st.gate_snapshots.entry(body.session_id)
        .or_insert_with(Vec::new)
        .push(entry);
    Json(serde_json::json!({"ok": true}))
}

#[derive(Deserialize)]
pub struct ToolCompletedBody {
    pub session_id: String,
}

pub async fn tool_completed(
    State(state): State<SharedState>,
    Json(body): Json<ToolCompletedBody>,
) -> impl IntoResponse {
    let st = state.lock().unwrap();
    crate::daemon::ws::emit_log(&st.event_tx, "info", "gate",
        format!("Tool completed for session {}", body.session_id));
    Json(serde_json::json!({"ok": true}))
}

#[derive(Serialize)]
pub struct DecisionResponse {
    pub status: String,
    pub snapshot_id: Option<String>,
    pub affected_paths: Vec<String>,
}

pub async fn get_decision(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let st = state.lock().unwrap();
    let decision = st.gate_decisions.get(&session_id);
    let snapshot_info = st.gate_snapshots.get(&session_id)
        .and_then(|snaps| snaps.last())
        .map(|s| (s.snapshot_id.as_str(), s.affected_paths.as_slice()));
    let resp = compute_decision_response(decision, snapshot_info);
    Json(resp)
}

pub async fn approve_gate(
    State(state): State<SharedState>,
    Path(gate_id): Path<String>,
) -> impl IntoResponse {
    match crate::daemon::gate::apply_decision(&state, &gate_id, true) {
        Ok(_) => Json(serde_json::json!({"ok": true, "approved": true})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

pub async fn reject_gate(
    State(state): State<SharedState>,
    Path(gate_id): Path<String>,
) -> impl IntoResponse {
    match crate::daemon::gate::apply_decision(&state, &gate_id, false) {
        Ok(_) => Json(serde_json::json!({"ok": true, "approved": false})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

// ============================================================================
// Fix 4 (2026-04-15): `no_session` sentinel so post-tool hook exits fast when
// the daemon has no record of this session. Previously returned "pending"
// indefinitely, causing the hook to poll for a full 60 seconds per tool call.
// ============================================================================

/// Pure: compute the decision response from (current decision state, most
/// recent snapshot). Extracted from `get_decision` for unit-testability.
///
/// Status semantics:
/// - `"approved"` / `"rejected"` — decision made, post-tool should act
/// - `"pending"` — snapshot exists but decision still being made, keep polling
/// - `"no_session"` — no snapshot and no decision, hook should exit immediately
pub(crate) fn compute_decision_response(
    decision: Option<&super::GateDecision>,
    latest_snapshot: Option<(&str, &[String])>,
) -> DecisionResponse {
    match (decision, latest_snapshot) {
        (Some(d), Some((snap_id, paths))) => DecisionResponse {
            status: if d.approved { "approved".to_string() } else { "rejected".to_string() },
            snapshot_id: Some(snap_id.to_string()),
            affected_paths: paths.to_vec(),
        },
        (Some(d), None) => DecisionResponse {
            status: if d.approved { "approved".to_string() } else { "rejected".to_string() },
            snapshot_id: None,
            affected_paths: Vec::new(),
        },
        (None, Some(_)) => DecisionResponse {
            status: "pending".to_string(),
            snapshot_id: None,
            affected_paths: Vec::new(),
        },
        (None, None) => DecisionResponse {
            status: "no_session".to_string(),
            snapshot_id: None,
            affected_paths: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_decision(approved: bool) -> super::super::GateDecision {
        super::super::GateDecision {
            approved,
            gate_id: "g1".to_string(),
            decided_at: std::time::Instant::now(),
        }
    }

    #[test]
    fn no_decision_no_snapshot_returns_no_session() {
        let resp = compute_decision_response(None, None);
        assert_eq!(resp.status, "no_session");
        assert!(resp.snapshot_id.is_none());
        assert!(resp.affected_paths.is_empty());
    }

    #[test]
    fn snapshot_but_no_decision_returns_pending() {
        let paths = vec!["foo.md".to_string()];
        let resp = compute_decision_response(None, Some(("snap1", &paths)));
        assert_eq!(resp.status, "pending");
    }

    #[test]
    fn approved_decision_returns_approved_with_snapshot() {
        let d = sample_decision(true);
        let paths = vec!["foo.md".to_string()];
        let resp = compute_decision_response(Some(&d), Some(("snap1", &paths)));
        assert_eq!(resp.status, "approved");
        assert_eq!(resp.snapshot_id.as_deref(), Some("snap1"));
        assert_eq!(resp.affected_paths, vec!["foo.md".to_string()]);
    }

    #[test]
    fn rejected_decision_returns_rejected_with_snapshot() {
        let d = sample_decision(false);
        let paths = vec!["foo.md".to_string()];
        let resp = compute_decision_response(Some(&d), Some(("snap1", &paths)));
        assert_eq!(resp.status, "rejected");
        assert_eq!(resp.snapshot_id.as_deref(), Some("snap1"));
    }

    #[test]
    fn decision_without_snapshot_returns_status_without_snapshot_fields() {
        let d = sample_decision(true);
        let resp = compute_decision_response(Some(&d), None);
        assert_eq!(resp.status, "approved");
        assert!(resp.snapshot_id.is_none());
        assert!(resp.affected_paths.is_empty());
    }
}
