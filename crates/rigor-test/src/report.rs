//! JSONL report generator -- reads harness event logs and writes HTML summaries.
//!
//! Input format matches `.harness/logs/harness-runs.jsonl`:
//! ```json
//! {"ts":"2026-04-23T21:48:39Z","tier":"tier-0","path":"src/main.rs","outcome":"pass","duration_ms":3200}
//! ```

use std::io::{BufRead, BufReader};

/// A single event from a harness JSONL log file.
#[derive(serde::Deserialize)]
struct HarnessEvent {
    ts: String,
    tier: String,
    path: String,
    outcome: String,
    duration_ms: u64,
    /// Optional extra metadata (forward-compatible with different extra shapes).
    #[allow(dead_code)]
    extra: Option<serde_json::Value>,
}

/// Read a JSONL events file and write an HTML summary report.
///
/// Malformed lines are logged to stderr and skipped (graceful degradation per ASVS V5).
pub fn run_report(input: std::path::PathBuf, output: std::path::PathBuf) -> anyhow::Result<()> {
    let file = std::fs::File::open(&input)?;
    let reader = BufReader::new(file);

    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<HarnessEvent>(&line) {
            Ok(ev) => events.push(ev),
            Err(e) => eprintln!("warn: skipping malformed line: {}", e),
        }
    }

    let total = events.len();
    let passed = events.iter().filter(|e| e.outcome == "pass").count();
    let failed = events.iter().filter(|e| e.outcome == "fail").count();
    let skipped = total.saturating_sub(passed + failed);
    let total_duration: u64 = events.iter().map(|e| e.duration_ms).sum();

    let rows: String = events
        .iter()
        .map(|e| {
            let css_class = match e.outcome.as_str() {
                "pass" => "pass",
                "fail" => "fail",
                _ => "skip",
            };
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"{}\">{}</td><td>{}ms</td></tr>",
                e.ts, e.tier, e.path, css_class, e.outcome, e.duration_ms
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        r#"<!DOCTYPE html>
<html><head><title>rigor-test Report</title>
<style>
body {{ font-family: system-ui; max-width: 900px; margin: 2em auto; }}
.pass {{ color: green; }} .fail {{ color: red; }} .skip {{ color: gray; }}
table {{ border-collapse: collapse; width: 100%; }}
td, th {{ border: 1px solid #ddd; padding: 8px; text-align: left; }}
</style></head><body>
<h1>rigor-test Report</h1>
<p>Input: <code>{input}</code></p>
<p>Total: {total} | <span class="pass">Pass: {passed}</span> | <span class="fail">Fail: {failed}</span> | <span class="skip">Skip: {skipped}</span> | Duration: {total_duration}ms</p>
<table><tr><th>Time</th><th>Tier</th><th>Path</th><th>Outcome</th><th>Duration</th></tr>
{rows}
</table></body></html>"#,
        input = input.display(),
        total = total,
        passed = passed,
        failed = failed,
        skipped = skipped,
        total_duration = total_duration,
        rows = rows
    );

    std::fs::write(&output, &html)?;
    println!("Report written to {}", output.display());
    Ok(())
}
