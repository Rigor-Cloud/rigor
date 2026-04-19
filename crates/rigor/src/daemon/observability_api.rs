//! REST API handlers for dashboard observability tabs.

use axum::extract::Query;
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::logging::session_registry;
use crate::logging::ViolationLogger;

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
