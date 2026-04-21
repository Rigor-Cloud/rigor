use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;

use super::ws::DaemonEvent;
use super::SharedState;

#[derive(serde::Serialize)]
pub struct ConstraintToggle {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub epistemic_type: String,
    pub strength: f64,
}

pub async fn list_constraints(State(state): State<SharedState>) -> impl IntoResponse {
    let st = state.lock().unwrap();
    let toggles: Vec<ConstraintToggle> = st
        .config
        .all_constraints()
        .into_iter()
        .map(|c| {
            let strength = st.graph.get_strength(&c.id).unwrap_or(0.0);
            let enabled = !st.disabled_constraints.contains(&c.id);
            ConstraintToggle {
                id: c.id.clone(),
                name: c.name.clone(),
                enabled,
                epistemic_type: format!("{:?}", c.epistemic_type).to_lowercase(),
                strength,
            }
        })
        .collect();
    Json(toggles)
}

#[derive(serde::Deserialize)]
pub struct ToggleBody {
    pub enabled: bool,
}

pub async fn toggle_constraint(
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<ToggleBody>,
) -> impl IntoResponse {
    let mut st = state.lock().unwrap();
    if body.enabled {
        st.disabled_constraints.remove(&id);
    } else {
        st.disabled_constraints.insert(id.clone());
    }
    let _ = st.event_tx.send(DaemonEvent::GovernanceState {
        action: "toggle_constraint".to_string(),
        detail: format!(
            "{} {}",
            id,
            if body.enabled { "enabled" } else { "disabled" }
        ),
    });
    Json(serde_json::json!({"ok": true, "id": id, "enabled": body.enabled}))
}

pub async fn toggle_pause(State(state): State<SharedState>) -> impl IntoResponse {
    let mut st = state.lock().unwrap();
    st.proxy_paused = !st.proxy_paused;
    let paused = st.proxy_paused;
    let _ = st.event_tx.send(DaemonEvent::GovernanceState {
        action: "pause".to_string(),
        detail: format!("proxy {}", if paused { "paused" } else { "resumed" }),
    });
    Json(serde_json::json!({"ok": true, "paused": paused}))
}

pub async fn toggle_block_next(State(state): State<SharedState>) -> impl IntoResponse {
    let mut st = state.lock().unwrap();
    st.block_next = !st.block_next;
    let block = st.block_next;
    let _ = st.event_tx.send(DaemonEvent::GovernanceState {
        action: "block_next".to_string(),
        detail: format!("block_next {}", if block { "armed" } else { "disarmed" }),
    });
    Json(serde_json::json!({"ok": true, "block_next": block}))
}
