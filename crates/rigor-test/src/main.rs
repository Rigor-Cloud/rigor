//! rigor-test — dev-only test orchestrator binary.
//!
//! D.1 ships with `--help` only. Subcommands (e2e, bench, report) are
//! implemented in Plan D.3.

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "rigor-test",
    about = "Dev-only test orchestrator for rigor. Not shipped to end users.",
    long_about = "Runs Layer 3 (real-agent E2E) and Layer 4 (token-economy benchmarks) \
                  test suites against the rigor harness and produces HTML reports. \
                  See docs/superpowers/specs/2026-04-15-test-harness-architecture-design.md."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// [D.3] Run real-agent E2E scenarios from a YAML suite.
    E2e {
        #[arg(long)]
        suite: Option<std::path::PathBuf>,
    },
    /// [D.3] Run token-economy + upgrade-claim benchmarks.
    Bench {
        #[arg(long)]
        suite: Option<std::path::PathBuf>,
        #[arg(long, default_value = "quick")]
        profile: String,
    },
    /// [D.3] Render an HTML report from a captured events JSONL file.
    Report {
        #[arg(long)]
        input: std::path::PathBuf,
        #[arg(long)]
        output: std::path::PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None => {
            eprintln!("rigor-test: dev-only test orchestrator. Use --help for subcommands.");
            Ok(())
        }
        Some(Commands::E2e { .. }) => {
            anyhow::bail!("rigor-test e2e: not yet implemented (ships in Plan D.3)")
        }
        Some(Commands::Bench { .. }) => {
            anyhow::bail!("rigor-test bench: not yet implemented (ships in Plan D.3)")
        }
        Some(Commands::Report { .. }) => {
            anyhow::bail!("rigor-test report: not yet implemented (ships in Plan D.3)")
        }
    }
}
