//! Benchmark dispatcher -- shells out to `cargo bench -p rigor`.
//!
//! Delegates to existing criterion benchmarks (hook_latency, evaluation_only,
//! dfquad_scaling, filter_chain_overhead) rather than reimplementing them.

use std::process::Command;

/// Run criterion benchmarks for the rigor crate.
///
/// - `suite`: optional path whose file stem selects a specific `--bench` target.
/// - `profile`: if "quick", appends `-- --quick` to reduce sample count.
pub fn run_bench(suite: Option<std::path::PathBuf>, profile: &str) -> anyhow::Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg("bench").arg("-p").arg("rigor");

    if let Some(ref suite_path) = suite {
        let bench_name = suite_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("hook_latency");
        cmd.arg("--bench").arg(bench_name);
    }

    if profile == "quick" {
        cmd.arg("--").arg("--quick");
    }

    let status = cmd.status()?;
    anyhow::ensure!(status.success(), "cargo bench exited with {}", status);
    Ok(())
}
