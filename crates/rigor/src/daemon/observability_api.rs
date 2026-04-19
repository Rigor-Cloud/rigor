//! REST API handlers for dashboard observability tabs.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json, Response};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::logging::session_registry;
use crate::logging::ViolationLogger;
use super::SharedState;

/// Walk up from `start` looking for `rigor.yaml`. Returns `None` when we
/// reach the filesystem root without finding one.
fn find_rigor_yaml_from(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join("rigor.yaml");
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

// ---------------------------------------------------------------------------
// GET /api/sessions — list all sessions
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SessionRow {
    id: String,
    name: String,
    agent: String,
    started_at: String,
    ended_at: Option<String>,
    alive: bool,
    constraints: usize,
    requests: Option<u64>,
    violations: Option<u64>,
    exit_code: Option<i32>,
}

pub async fn list_sessions() -> Response {
    let sessions = session_registry::read_all_sessions().unwrap_or_default();
    let rows: Vec<SessionRow> = sessions
        .iter()
        .rev()
        .take(50)
        .map(|s| SessionRow {
            id: s.id.clone(),
            name: s.name.clone(),
            agent: s.agent.clone(),
            started_at: s.started_at.clone(),
            ended_at: s.ended_at.clone(),
            alive: session_registry::is_session_alive(s),
            constraints: s.constraints,
            requests: s.requests,
            violations: s.violations,
            exit_code: s.exit_code,
        })
        .collect();
    Json(rows).into_response()
}

// ---------------------------------------------------------------------------
// GET /api/violations?q=&constraint=&severity=&limit=
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ViolationQuery {
    q: Option<String>,
    constraint: Option<String>,
    severity: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct ViolationRow {
    timestamp: String,
    session_id: String,
    constraint_id: String,
    constraint_name: String,
    severity: String,
    decision: String,
    message: String,
    claim_text: Vec<String>,
    model: Option<String>,
}

pub async fn search_violations(Query(params): Query<ViolationQuery>) -> Response {
    let logger = match ViolationLogger::new() {
        Ok(l) => l,
        Err(_) => return Json(Vec::<ViolationRow>::new()).into_response(),
    };
    let entries = logger.read_all().unwrap_or_default();
    let limit = params.limit.unwrap_or(100);

    let rows: Vec<ViolationRow> = entries
        .iter()
        .rev()
        .filter(|e| {
            if let Some(ref q) = params.q {
                let q_lower = q.to_lowercase();
                let matches = e.constraint_id.to_lowercase().contains(&q_lower)
                    || e.message.to_lowercase().contains(&q_lower)
                    || e.claim_text.iter().any(|c| c.to_lowercase().contains(&q_lower));
                if !matches {
                    return false;
                }
            }
            if let Some(ref cid) = params.constraint {
                if e.constraint_id != *cid {
                    return false;
                }
            }
            if let Some(ref sev) = params.severity {
                if e.severity != *sev {
                    return false;
                }
            }
            true
        })
        .take(limit)
        .map(|e| ViolationRow {
            timestamp: e.session.timestamp.clone(),
            session_id: e.session.session_id.clone(),
            constraint_id: e.constraint_id.clone(),
            constraint_name: e.constraint_name.clone(),
            severity: e.severity.clone(),
            decision: e.decision.clone(),
            message: e.message.clone(),
            claim_text: e.claim_text.clone(),
            model: e.model.clone(),
        })
        .collect();

    Json(rows).into_response()
}

// ---------------------------------------------------------------------------
// GET /api/eval — constraint effectiveness stats
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct EvalStats {
    total_violations: usize,
    total_sessions: usize,
    violations_per_session: f64,
    false_positive_count: usize,
    precision: f64,
    constraints: Vec<ConstraintStat>,
}

#[derive(Serialize)]
struct ConstraintStat {
    id: String,
    name: String,
    hits: usize,
    false_positives: usize,
    fp_rate: f64,
    last_fired: Option<String>,
}

pub async fn eval_stats() -> Response {
    let logger = match ViolationLogger::new() {
        Ok(l) => l,
        Err(_) => {
            return Json(EvalStats {
                total_violations: 0,
                total_sessions: 0,
                violations_per_session: 0.0,
                false_positive_count: 0,
                precision: 0.0,
                constraints: Vec::new(),
            })
            .into_response()
        }
    };
    let entries = logger.read_all().unwrap_or_default();

    let total = entries.len();
    let sessions: std::collections::HashSet<_> =
        entries.iter().map(|e| &e.session.session_id).collect();
    let session_count = sessions.len().max(1);
    let fp_count = entries.iter().filter(|e| e.false_positive == Some(true)).count();
    let annotated = entries.iter().filter(|e| e.false_positive.is_some()).count();
    let precision = if annotated > 0 {
        ((annotated - fp_count) as f64 / annotated as f64) * 100.0
    } else {
        0.0
    };

    // Per-constraint stats
    let mut constraint_map: HashMap<String, (String, usize, usize, Option<String>)> =
        HashMap::new();
    for e in &entries {
        let entry = constraint_map
            .entry(e.constraint_id.clone())
            .or_insert_with(|| (e.constraint_name.clone(), 0, 0, None));
        entry.1 += 1;
        if e.false_positive == Some(true) {
            entry.2 += 1;
        }
        entry.3 = Some(e.session.timestamp.clone());
    }

    let mut constraints: Vec<ConstraintStat> = constraint_map
        .into_iter()
        .map(|(id, (name, hits, fps, last))| ConstraintStat {
            id,
            name,
            hits,
            false_positives: fps,
            fp_rate: if hits > 0 {
                (fps as f64 / hits as f64) * 100.0
            } else {
                0.0
            },
            last_fired: last,
        })
        .collect();
    constraints.sort_by(|a, b| b.hits.cmp(&a.hits));

    Json(EvalStats {
        total_violations: total,
        total_sessions: session_count,
        violations_per_session: total as f64 / session_count as f64,
        false_positive_count: fp_count,
        precision,
        constraints,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// GET /api/cost — cumulative session cost tracking
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct CostModelBreakdown {
    model: String,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd: f64,
}

#[derive(Serialize)]
struct CostResponse {
    total_input_tokens: u64,
    total_output_tokens: u64,
    total_cost_usd: f64,
    max_cost_usd: Option<f64>,
    budget_exceeded: bool,
    proxy_paused: bool,
    cost_per_violation: f64,
    models: Vec<CostModelBreakdown>,
}

pub async fn cost_stats(State(state): State<SharedState>) -> Response {
    let st = state.lock().unwrap();

    // Count violations for cost-per-violation
    let violation_count = {
        let logger = ViolationLogger::new().ok();
        logger.and_then(|l| l.read_all().ok()).map(|e| e.len()).unwrap_or(0)
    };
    let cost_per_violation = if violation_count > 0 {
        st.cumulative_cost_usd / violation_count as f64
    } else {
        0.0
    };

    let models: Vec<CostModelBreakdown> = st.cost_by_model.iter()
        .map(|(model, (inp, out, cost))| CostModelBreakdown {
            model: model.clone(),
            input_tokens: *inp,
            output_tokens: *out,
            cost_usd: *cost,
        })
        .collect();

    let budget_exceeded = st.max_cost_usd
        .map(|max| st.cumulative_cost_usd > max)
        .unwrap_or(false);

    Json(CostResponse {
        total_input_tokens: st.cumulative_input_tokens,
        total_output_tokens: st.cumulative_output_tokens,
        total_cost_usd: st.cumulative_cost_usd,
        max_cost_usd: st.max_cost_usd,
        budget_exceeded,
        proxy_paused: st.proxy_paused,
        cost_per_violation,
        models,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// POST /api/project/register — per-project constraint discovery
// ---------------------------------------------------------------------------
//
// Invoked by the OpenCode plugin's `session.created` hook. The plugin POSTs
// the active project directory and we walk up from there looking for a
// `rigor.yaml`. If one is found AND it's different from whatever we already
// have loaded, we rebuild the argumentation graph + policy engine in place
// so the next proxied LLM request evaluates against this project's rules.
//
// If no rigor.yaml exists in the tree, the daemon keeps its current state
// (typically "empty" / zero constraints from `rigor serve`). Errors in
// loading are non-fatal for the session — we just return a 400 so the
// plugin can log it.

#[derive(Deserialize)]
pub struct RegisterProjectBody {
    pub directory: String,
}

#[derive(Serialize)]
pub struct RegisterProjectResponse {
    pub constraints: usize,
    pub path: String,
    /// "loaded" — newly loaded this call.
    /// "cached" — same path was already loaded, no reload happened.
    /// "none" — no rigor.yaml found in the directory tree.
    pub status: String,
}

pub async fn register_project(
    State(state): State<SharedState>,
    Json(body): Json<RegisterProjectBody>,
) -> Response {
    let start = PathBuf::from(&body.directory);

    // Walk up looking for rigor.yaml. Missing file is a legitimate outcome
    // (project isn't using rigor) — tell the caller so it can decide whether
    // to log, not an error.
    let Some(yaml_path) = find_rigor_yaml_from(&start) else {
        return Json(RegisterProjectResponse {
            constraints: 0,
            path: String::new(),
            status: "none".to_string(),
        })
        .into_response();
    };

    // Cache check before we grab the write lock: if nothing changed we can
    // take the short read-lock path and skip the graph rebuild entirely.
    {
        let st = state.lock().unwrap();
        if st.yaml_path == yaml_path {
            return Json(RegisterProjectResponse {
                constraints: st.config.all_constraints().len(),
                path: yaml_path.display().to_string(),
                status: "cached".to_string(),
            })
            .into_response();
        }
    }

    let path_display = yaml_path.display().to_string();
    let result = {
        let mut st = state.lock().unwrap();
        st.reload_config(yaml_path)
    };

    match result {
        Ok(count) => {
            eprintln!(
                "rigor daemon: hot-reloaded {} constraints from {}",
                count, path_display
            );
            Json(RegisterProjectResponse {
                constraints: count,
                path: path_display,
                status: "loaded".to_string(),
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("failed to load {}: {}", path_display, e),
            })),
        )
            .into_response(),
    }
}
