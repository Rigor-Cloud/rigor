use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::logging::{ViolationLogEntry, ViolationLogger};

/// Summary of a single past session: what constraints fired, representative claims, outcome.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionEpisode {
    pub session_id: String,
    pub timestamp: String,
    pub git_commit: Option<String>,
    pub total_violations: usize,
    pub block_count: usize,
    pub warn_count: usize,
    pub constraint_ids: Vec<String>,
    /// A few representative claim snippets that were flagged (non-FP preferred).
    pub sample_claims: Vec<String>,
    /// Human outcome classification derived from annotations.
    /// "verified" if any TP, "false_positive_dominant" if majority FP, "unreviewed" otherwise.
    pub outcome: String,
}

/// Episodic memory: ordered list of session episodes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EpisodicMemory {
    pub episodes: Vec<SessionEpisode>,
}

/// Aggregated counts of which constraints tend to fire on which file paths.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PathPattern {
    /// file path or transcript path fragment
    pub path: String,
    /// constraint_id -> hit count
    pub constraint_hits: HashMap<String, usize>,
    pub total_hits: usize,
}

/// Aggregated counts of which models produce which types of false claims.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelPattern {
    pub model: String,
    /// constraint_id -> false-positive or confirmed counts
    pub constraint_hits: HashMap<String, usize>,
    pub false_positives: usize,
    pub true_positives: usize,
}

/// Semantic memory about this codebase / the models we've observed.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SemanticMemory {
    pub paths: HashMap<String, PathPattern>,
    pub models: HashMap<String, ModelPattern>,
}

/// Combined on-disk memory store.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryStore {
    pub episodic: EpisodicMemory,
    pub semantic: SemanticMemory,
    pub last_updated: Option<String>,
}

impl MemoryStore {
    pub fn path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Failed to get home directory")?;
        let dir = home.join(".rigor");
        fs::create_dir_all(&dir).ok();
        Ok(dir.join("memory.json"))
    }

    /// Load memory from `~/.rigor/memory.json`. Returns an empty store if missing.
    pub fn load() -> Result<Self> {
        let p = Self::path()?;
        if !p.exists() {
            return Ok(Self::default());
        }
        let s = fs::read_to_string(&p)?;
        let store: MemoryStore = serde_json::from_str(&s).unwrap_or_default();
        Ok(store)
    }

    pub fn save(&self) -> Result<PathBuf> {
        let p = Self::path()?;
        let mut copy = self.clone();
        copy.last_updated = Some(chrono::Utc::now().to_rfc3339());
        let json = serde_json::to_string_pretty(&copy)?;
        fs::write(&p, json)?;
        Ok(p)
    }

    /// Rebuild memory deterministically from the current violation log.
    /// This is the easiest way to keep memory consistent; callers can invoke
    /// this opportunistically.
    pub fn rebuild_from_log() -> Result<Self> {
        let logger = ViolationLogger::new()?;
        let entries = logger.read_all()?;
        Ok(Self::from_entries(&entries))
    }

    pub fn from_entries(entries: &[ViolationLogEntry]) -> Self {
        let mut episodic: HashMap<String, SessionEpisode> = HashMap::new();
        let mut semantic = SemanticMemory::default();

        // Aggregate annotation signals per session to decide outcome.
        let mut fp_count: HashMap<String, usize> = HashMap::new();
        let mut tp_count: HashMap<String, usize> = HashMap::new();

        for e in entries {
            let ep = episodic
                .entry(e.session.session_id.clone())
                .or_insert_with(|| SessionEpisode {
                    session_id: e.session.session_id.clone(),
                    timestamp: e.session.timestamp.clone(),
                    git_commit: e.session.git_commit.clone(),
                    total_violations: 0,
                    block_count: 0,
                    warn_count: 0,
                    constraint_ids: Vec::new(),
                    sample_claims: Vec::new(),
                    outcome: "unreviewed".to_string(),
                });
            ep.total_violations += 1;
            match e.severity.as_str() {
                "block" => ep.block_count += 1,
                "warn" => ep.warn_count += 1,
                _ => {}
            }
            if !ep.constraint_ids.contains(&e.constraint_id) {
                ep.constraint_ids.push(e.constraint_id.clone());
            }
            if ep.sample_claims.len() < 3 {
                if let Some(t) = e.claim_text.first() {
                    let clip: String = t.chars().take(160).collect();
                    ep.sample_claims.push(clip);
                }
            }
            match e.false_positive {
                Some(true) => *fp_count.entry(e.session.session_id.clone()).or_insert(0) += 1,
                Some(false) => *tp_count.entry(e.session.session_id.clone()).or_insert(0) += 1,
                None => {}
            }

            // Semantic: paths
            if let Some(p) = &e.transcript_path {
                let pp = semantic
                    .paths
                    .entry(p.clone())
                    .or_insert_with(|| PathPattern {
                        path: p.clone(),
                        ..Default::default()
                    });
                *pp.constraint_hits
                    .entry(e.constraint_id.clone())
                    .or_insert(0) += 1;
                pp.total_hits += 1;
            }

            // Semantic: models — we don't have a direct "model" field; use
            // claim_type as a coarse proxy. If callers later populate a
            // model field, semantic memory can be extended without schema change.
            let model_key = e
                .claim_type
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            let mp = semantic
                .models
                .entry(model_key.clone())
                .or_insert_with(|| ModelPattern {
                    model: model_key,
                    ..Default::default()
                });
            *mp.constraint_hits
                .entry(e.constraint_id.clone())
                .or_insert(0) += 1;
            match e.false_positive {
                Some(true) => mp.false_positives += 1,
                Some(false) => mp.true_positives += 1,
                None => {}
            }
        }

        for (sid, ep) in episodic.iter_mut() {
            let fp = *fp_count.get(sid).unwrap_or(&0);
            let tp = *tp_count.get(sid).unwrap_or(&0);
            ep.outcome = if tp > 0 && tp >= fp {
                "verified".to_string()
            } else if fp > 0 && fp > tp {
                "false_positive_dominant".to_string()
            } else {
                "unreviewed".to_string()
            };
        }

        let mut episodes: Vec<SessionEpisode> = episodic.into_values().collect();
        episodes.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        MemoryStore {
            episodic: EpisodicMemory { episodes },
            semantic,
            last_updated: Some(chrono::Utc::now().to_rfc3339()),
        }
    }

    /// Return up to `n` most recent episodes (most recent first).
    pub fn recent_episodes(&self, n: usize) -> Vec<&SessionEpisode> {
        let len = self.episodic.episodes.len();
        let start = len.saturating_sub(n);
        self.episodic.episodes[start..].iter().rev().collect()
    }

    /// Return top-N constraints that have fired most frequently across all sessions,
    /// excluding episodes marked as `false_positive_dominant`.
    pub fn top_relevant_constraints(&self, n: usize) -> Vec<(String, usize)> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for ep in &self.episodic.episodes {
            if ep.outcome == "false_positive_dominant" {
                continue;
            }
            for cid in &ep.constraint_ids {
                *counts.entry(cid.clone()).or_insert(0) += 1;
            }
        }
        let mut v: Vec<(String, usize)> = counts.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.truncate(n);
        v
    }

    /// Return a human-readable warning for a given model key if its FP count
    /// dominates, e.g. "assertion: tends to produce false positives on c1".
    pub fn model_warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for mp in self.semantic.models.values() {
            if mp.false_positives == 0 && mp.true_positives == 0 {
                continue;
            }
            let total = mp.false_positives + mp.true_positives;
            if total < 3 {
                continue;
            }
            let fp_rate = mp.false_positives as f64 / total as f64;
            if fp_rate > 0.4 {
                // top constraint for this model
                let mut top: Vec<(&String, &usize)> = mp.constraint_hits.iter().collect();
                top.sort_by(|a, b| b.1.cmp(a.1));
                let cid = top.first().map(|(k, _)| k.as_str()).unwrap_or("?");
                out.push(format!(
                    "model-signal `{}` has high false-positive rate ({:.0}%) — most common on `{}`",
                    mp.model,
                    fp_rate * 100.0,
                    cid
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::types::SessionMetadata;

    fn mk(sid: &str, cid: &str, fp: Option<bool>, path: Option<&str>) -> ViolationLogEntry {
        ViolationLogEntry {
            session: SessionMetadata {
                session_id: sid.into(),
                timestamp: "2026-04-19T00:00:00Z".into(),
                git_commit: Some("deadbeef".into()),
                git_dirty: false,
            },
            constraint_id: cid.into(),
            constraint_name: format!("n_{}", cid),
            claim_ids: vec!["c1".into()],
            claim_text: vec!["sample claim text".into()],
            base_strength: 0.8,
            computed_strength: 0.8,
            severity: "block".into(),
            decision: "block".into(),
            message: "m".into(),
            supporters: vec![],
            attackers: vec![],
            total_claims: 1,
            total_constraints: 1,
            transcript_path: path.map(|s| s.to_string()),
            claim_confidence: None,
            claim_type: Some("assertion".into()),
            claim_source: None,
            false_positive: fp,
            annotation_note: None,
            model: None,
        }
    }

    #[test]
    fn test_from_entries_builds_episodes_and_semantic() {
        let entries = vec![
            mk("s1", "c1", Some(false), Some("/tmp/t1.json")),
            mk("s1", "c2", Some(true), Some("/tmp/t1.json")),
            mk("s2", "c1", None, Some("/tmp/t2.json")),
        ];
        let store = MemoryStore::from_entries(&entries);
        assert_eq!(store.episodic.episodes.len(), 2);
        assert_eq!(store.semantic.paths.len(), 2);

        let s1 = store
            .episodic
            .episodes
            .iter()
            .find(|e| e.session_id == "s1")
            .unwrap();
        assert_eq!(s1.outcome, "verified");
        assert_eq!(s1.total_violations, 2);

        let warns = store.model_warnings();
        // Not enough samples (3) to trigger; should be empty.
        assert!(warns.is_empty());
    }

    #[test]
    fn test_recent_episodes_ordering() {
        let mut entries = Vec::new();
        for i in 0..5 {
            let mut e = mk(&format!("s{}", i), "c1", Some(false), None);
            e.session.timestamp = format!("2026-04-{:02}T00:00:00Z", 10 + i);
            entries.push(e);
        }
        let store = MemoryStore::from_entries(&entries);
        let recent = store.recent_episodes(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].session_id, "s4"); // most recent first
    }
}
