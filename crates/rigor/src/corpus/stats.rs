//! Aggregate corpus statistics — feeds `rigor corpus stats` and dashboard
//! surfaces. Pure function over `{prompt_id: {model: [RecordedSample]}}`
//! and a replay function that turns each sample into a "did rigor block?"
//! boolean.

use std::collections::BTreeMap;

use super::recording::RecordedSample;

/// A per-(prompt, model) block-rate observation.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelStats {
    pub prompt_id: String,
    pub model: String,
    pub samples: u32,
    pub blocks: u32,
}

impl ModelStats {
    pub fn block_rate(&self) -> f64 {
        if self.samples == 0 {
            0.0
        } else {
            self.blocks as f64 / self.samples as f64
        }
    }
}

/// Compute per-model block-rates by replaying every sample through `replay_fn`.
///
/// `replay_fn` returns `true` when rigor's decision was block for the given
/// sample. Decoupling the replay function keeps this module free of any
/// direct dependency on the PolicyEngine — tests can inject a deterministic
/// oracle.
pub fn compute_stats<F>(
    recordings: &BTreeMap<String, BTreeMap<String, Vec<RecordedSample>>>,
    mut replay_fn: F,
) -> Vec<ModelStats>
where
    F: FnMut(&RecordedSample) -> bool,
{
    let mut out = Vec::new();
    for (prompt_id, per_model) in recordings {
        for (model, samples) in per_model {
            let blocks = samples.iter().filter(|s| replay_fn(s)).count() as u32;
            out.push(ModelStats {
                prompt_id: prompt_id.clone(),
                model: model.clone(),
                samples: samples.len() as u32,
                blocks,
            });
        }
    }
    out.sort_by(|a, b| a.prompt_id.cmp(&b.prompt_id).then(a.model.cmp(&b.model)));
    out
}

/// Collapse per-prompt rows into per-model aggregates.
#[derive(Debug, Clone, PartialEq)]
pub struct PerModelAggregate {
    pub model: String,
    pub total_samples: u32,
    pub total_blocks: u32,
}

impl PerModelAggregate {
    pub fn block_rate(&self) -> f64 {
        if self.total_samples == 0 {
            0.0
        } else {
            self.total_blocks as f64 / self.total_samples as f64
        }
    }
}

pub fn aggregate_by_model(rows: &[ModelStats]) -> Vec<PerModelAggregate> {
    let mut by_model: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    for row in rows {
        let entry = by_model.entry(row.model.clone()).or_insert((0, 0));
        entry.0 += row.samples;
        entry.1 += row.blocks;
    }
    by_model
        .into_iter()
        .map(|(model, (s, b))| PerModelAggregate {
            model,
            total_samples: s,
            total_blocks: b,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::recording::TokenCounts;
    use chrono::Utc;

    fn sample(prompt: &str, model: &str, idx: u32, text: &str) -> RecordedSample {
        RecordedSample {
            prompt_id: prompt.into(),
            prompt_hash: "sha256:t".into(),
            model: model.into(),
            sample_index: idx,
            recorded_at: Utc::now(),
            temperature: 0.7,
            response_text: text.into(),
            tokens: TokenCounts {
                prompt: 1,
                completion: 2,
            },
            cost_usd: None,
            openrouter_response_id: None,
        }
    }

    #[test]
    fn compute_stats_counts_blocks_per_prompt_model() {
        let mut recordings: BTreeMap<String, BTreeMap<String, Vec<RecordedSample>>> =
            BTreeMap::new();
        recordings.entry("p1".into()).or_default().insert(
            "model-a".into(),
            vec![
                sample("p1", "model-a", 0, "BLOCK"),
                sample("p1", "model-a", 1, "ok"),
                sample("p1", "model-a", 2, "BLOCK"),
            ],
        );
        recordings
            .entry("p1".into())
            .or_default()
            .insert("model-b".into(), vec![sample("p1", "model-b", 0, "ok")]);

        // Oracle: block iff response contains "BLOCK".
        let rows = compute_stats(&recordings, |s| s.response_text.contains("BLOCK"));
        assert_eq!(rows.len(), 2);

        let a = rows.iter().find(|r| r.model == "model-a").unwrap();
        assert_eq!(a.samples, 3);
        assert_eq!(a.blocks, 2);
        assert!((a.block_rate() - 2.0 / 3.0).abs() < 1e-9);

        let b = rows.iter().find(|r| r.model == "model-b").unwrap();
        assert_eq!(b.samples, 1);
        assert_eq!(b.blocks, 0);
        assert_eq!(b.block_rate(), 0.0);
    }

    #[test]
    fn aggregate_by_model_sums_across_prompts() {
        let rows = vec![
            ModelStats {
                prompt_id: "p1".into(),
                model: "m".into(),
                samples: 10,
                blocks: 7,
            },
            ModelStats {
                prompt_id: "p2".into(),
                model: "m".into(),
                samples: 10,
                blocks: 3,
            },
            ModelStats {
                prompt_id: "p1".into(),
                model: "n".into(),
                samples: 5,
                blocks: 5,
            },
        ];
        let agg = aggregate_by_model(&rows);
        let m = agg.iter().find(|a| a.model == "m").unwrap();
        assert_eq!(m.total_samples, 20);
        assert_eq!(m.total_blocks, 10);
        assert_eq!(m.block_rate(), 0.5);
        let n = agg.iter().find(|a| a.model == "n").unwrap();
        assert_eq!(n.total_samples, 5);
        assert_eq!(n.total_blocks, 5);
        assert_eq!(n.block_rate(), 1.0);
    }

    #[test]
    fn compute_stats_empty_input_yields_empty_output() {
        let recordings: BTreeMap<String, BTreeMap<String, Vec<RecordedSample>>> = BTreeMap::new();
        let rows = compute_stats(&recordings, |_| true);
        assert!(rows.is_empty());
    }
}
