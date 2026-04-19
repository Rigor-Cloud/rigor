pub mod alert;
pub mod config;
pub mod diff;
pub mod eval;
pub mod gate;
pub mod graph;
pub mod ground;
pub mod init;
pub mod log;
pub mod logs;
pub mod map;
pub mod refine;
pub mod scan;
pub mod search;
pub mod serve;
pub mod sessions;
pub mod setup;
pub mod show;
pub mod validate;
pub mod web;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "rigor",
    about = "Epistemic constraint enforcement for LLM outputs"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize rigor.yaml for a project (detects language, dependencies)
    Init {
        /// Path to project directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Use AI to analyze the project and generate targeted constraints
        #[arg(long)]
        ai: bool,
    },
    /// Display all constraints with strengths and severity zones
    Show {
        /// Path to rigor.yaml (searches current directory tree if not provided)
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
    /// Validate rigor.yaml configuration
    Validate {
        /// Path to rigor.yaml (searches current directory tree if not provided)
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
    /// Output constraint graph in DOT format for Graphviz
    Graph {
        /// Path to rigor.yaml (searches current directory tree if not provided)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Launch interactive 3D graph explorer in browser (localhost only)
        #[arg(long)]
        web: bool,

        /// Port for the web explorer (default: 8484)
        #[arg(long, default_value = "8484")]
        port: u16,
    },
    /// Epistemically ground an AI process (proxy + knowledge graph)
    Ground {
        /// Path to rigor.yaml
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Daemon port
        #[arg(long, default_value = "8787")]
        port: u16,

        /// Show daemon logs in terminal (default is quiet — logs go to /tmp/rigor-ground.log)
        #[arg(long)]
        show_logs: bool,

        /// Disable TLS MITM for LLM endpoints. By default rigor terminates
        /// TLS to inspect requests, inject epistemic context, and extract
        /// claims. Pass --no-mitm to use blind-tunnel mode (observe only).
        #[arg(long)]
        no_mitm: bool,

        /// Transparent interception mode (mirrord-style). Instead of setting
        /// HTTPS_PROXY env vars, the layer's connect() hook redirects ALL
        /// outbound port 443 connections to rigor's TLS listener. The daemon
        /// peeks the TLS ClientHello SNI to determine the real target host
        /// and decides whether to MITM (LLM endpoints) or blind-tunnel.
        ///
        /// Use this for clients (like Claude Code) that disable OAuth when
        /// they detect HTTPS_PROXY in the environment.
        #[arg(short, long)]
        transparent: bool,

        /// Human-friendly session name (auto-generated if not provided)
        #[arg(long)]
        name: Option<String>,

        /// Maximum session cost in USD. When total estimated cost exceeds
        /// this value, the proxy is paused automatically.
        #[arg(long)]
        max_cost: Option<f64>,

        /// Command to ground (everything after --)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
    /// Start the rigor daemon without spawning a process
    Daemon {
        /// Path to rigor.yaml
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Port to listen on
        #[arg(long, default_value = "8787")]
        port: u16,
    },
    /// Run rigor as a persistent background daemon that any LLM tool can
    /// proxy through. Unlike `rigor ground`, this does not wrap a child
    /// process — clients connect by setting HTTPS_PROXY=http://127.0.0.1:<port>.
    ///
    /// Subcommand `rigor serve stop` kills the background daemon.
    Serve {
        /// Optional positional action: "stop" to kill the background daemon.
        action: Option<String>,

        /// Path to rigor.yaml (searches current directory tree if not provided)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Port to listen on
        #[arg(long, default_value = "8787")]
        port: u16,

        /// Daemonize: fork into the background and write PID to ~/.rigor/serve.pid
        #[arg(long)]
        background: bool,

        /// Equivalent to `rigor serve stop` — stop the running background daemon.
        #[arg(long, conflicts_with = "background")]
        stop: bool,

        /// Human-friendly session name (auto-generated if not provided)
        #[arg(long)]
        name: Option<String>,

        /// Maximum session cost in USD. When total estimated cost exceeds
        /// this value, the proxy is paused automatically.
        #[arg(long)]
        max_cost: Option<f64>,
    },
    /// Query and annotate violation logs
    Log {
        #[command(subcommand)]
        command: log::LogCommands,
    },
    /// List grounding sessions
    Sessions {
        /// Only show active (running) sessions
        #[arg(long)]
        active: bool,
        /// Show last N sessions (default 10)
        #[arg(long)]
        last: Option<usize>,
    },
    /// View session logs
    Logs {
        /// Session name or ID to view (default: latest)
        #[arg(short, long)]
        session: Option<String>,
        /// Follow log output in real-time (like tail -f)
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show (default 50)
        #[arg(short = 'n', long, default_value = "50")]
        lines: usize,
    },
    /// Install the rigor CA certificate into the macOS login keychain.
    /// After this, ALL apps trust rigor's MITM certificates — no more
    /// NODE_TLS_REJECT_UNAUTHORIZED=0 needed.
    Trust,
    /// Remove the rigor CA certificate from the macOS login keychain.
    Untrust,
    /// Configure rigor global settings (judge API, model, etc.)
    Config {
        /// Action: set, get, list
        action: String,
        /// Config key (e.g. judge.api_key)
        key: Option<String>,
        /// Config value
        value: Option<String>,
    },
    /// Verify source anchors and track code-grounded constraints.
    /// Uses LSP for deep semantic analysis (--deep) or grep for fast checks.
    Map {
        /// Path to rigor.yaml
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Path to the codebase root (defaults to current directory)
        #[arg(long)]
        codebase: Option<PathBuf>,

        /// Just verify existing anchors, don't generate new constraints
        #[arg(long)]
        check: bool,

        /// Use LSP for deep semantic analysis (finds all references, types, overrides)
        #[arg(long)]
        deep: bool,
    },
    /// Action gate subcommands (used by Claude Code hooks)
    Gate {
        subcommand: String,
    },
    /// Scan stdin (or a file) for PII/secrets using rigor's detector.
    /// Default: read stdin, print findings. Use `--hook` to run as a
    /// UserPromptSubmit hook (reads JSON input, emits hook response JSON).
    /// Use `--install` / `--uninstall` / `--status` to manage the hook.
    Scan {
        /// Read from a file instead of stdin. Use "-" for stdin explicitly.
        #[arg(short, long)]
        file: Option<String>,

        /// Exit with status 1 if any PII/secrets are detected.
        #[arg(long)]
        block: bool,

        /// Emit findings as JSON on stdout instead of the human-readable
        /// summary on stderr.
        #[arg(long)]
        json: bool,

        /// Run in UserPromptSubmit hook mode: read hook JSON from stdin,
        /// scan the user's prompt, emit a hook response. Block if any PII.
        #[arg(long, conflicts_with_all = ["install", "uninstall", "status"])]
        hook: bool,

        /// Install `rigor scan --hook` as a UserPromptSubmit hook in
        /// ~/.claude/settings.json.
        #[arg(long, conflicts_with_all = ["uninstall", "status", "hook"])]
        install: bool,

        /// Remove the rigor scan UserPromptSubmit hook from settings.
        #[arg(long, conflicts_with_all = ["install", "status", "hook"])]
        uninstall: bool,

        /// Report whether the rigor scan UserPromptSubmit hook is installed.
        #[arg(long, conflicts_with_all = ["install", "uninstall", "hook"])]
        status: bool,

        /// Apply local entropy + context heuristic filtering after regex
        /// detection to drop likely false positives (version numbers,
        /// UUIDs, commit hashes, example values). Zero token cost.
        #[arg(long)]
        smart: bool,
    },
    /// Full-text search across all violations and session logs.
    Search {
        /// Query to match against claim_text, constraint_id, constraint_name, and message.
        #[arg(value_name = "QUERY")]
        query: Option<String>,

        /// Only return violations of this constraint id.
        #[arg(long)]
        constraint: Option<String>,

        /// Only return violations with this severity (block|warn|allow).
        #[arg(long)]
        severity: Option<String>,

        /// Only return violations at or after this date (YYYY-MM-DD or RFC3339).
        #[arg(long)]
        since: Option<String>,

        /// Only return violations whose model identifier contains this substring.
        #[arg(long)]
        model: Option<String>,

        /// Maximum number of matches to display.
        #[arg(long, default_value = "50")]
        limit: usize,
    },
    /// Configure webhook alerts fired when violations are detected.
    Alert {
        #[command(subcommand)]
        command: alert::AlertCommands,
    },
    /// Compare two sessions' violation patterns.
    Diff {
        /// First session identifier (name or id prefix).
        session_a: Option<String>,
        /// Second session identifier (name or id prefix).
        session_b: Option<String>,
        /// Compare the last N sessions (currently supports 2).
        #[arg(long)]
        last: Option<usize>,
    },
    /// Evaluate constraint effectiveness against the violation log.
    /// Computes precision, recall trend, and per-constraint hit rates.
    Eval {
        /// Write a markdown report to .rigor/eval-report.md
        #[arg(long)]
        report: bool,
        /// Save current metrics as the baseline at ~/.rigor/eval-baseline.json
        #[arg(long)]
        baseline: bool,
        /// Compare current metrics against saved baseline
        #[arg(long)]
        compare: bool,
    },
    /// Analyze violation patterns and suggest constraint refinements.
    /// Targets constraints with false-positive rate above 30%.
    Refine {
        /// Apply suggested refinements directly to rigor.yaml
        #[arg(long)]
        apply: bool,
        /// Print the diff that would be applied, without modifying rigor.yaml
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Interactive setup wizard for non-engineers.
    /// Walks through: daemon start, rigor.yaml init, OpenCode plugin install,
    /// and a health check against the running daemon.
    Setup,
}

/// Run the CLI. If no subcommand is given, fall through to hook mode.
pub fn run_cli() -> Result<()> {
    // When invoked as a Claude Code stop hook, stdin has JSON piped in and there are no args.
    // clap would show help on no args if command were required, but it's Option so it parses fine.
    let cli = Cli::parse();

    match cli.command {
        None => {
            // No subcommand: run as Claude Code stop hook (original behavior)
            crate::run()
        }
        Some(Commands::Init { path, ai }) => init::run_init(path, ai),
        Some(Commands::Ground { path, port, show_logs, no_mitm, transparent, name, max_cost, command }) => ground::run_ground(path, port, !show_logs, !no_mitm, transparent, name, max_cost, command),
        Some(Commands::Daemon { path, port }) => crate::daemon::start_daemon(path, port),
        Some(Commands::Serve { action, path, port, background, stop, name, max_cost }) => {
            let stop = stop || matches!(action.as_deref(), Some("stop"));
            serve::run_serve(path, port, background, stop, name, max_cost)
        }
        Some(Commands::Show { path }) => show::run_show(path),
        Some(Commands::Validate { path }) => validate::run_validate(path),
        Some(Commands::Graph { path, web, port }) => {
            if web {
                web::run_web(path, port)
            } else {
                graph::run_graph(path)
            }
        }
        Some(Commands::Log { command }) => log::run_log(command),
        Some(Commands::Sessions { active, last }) => sessions::run_sessions(active, last),
        Some(Commands::Logs { session, follow, lines }) => logs::run_logs(session, follow, lines),
        Some(Commands::Trust) => crate::daemon::tls::install_ca_trust(),
        Some(Commands::Untrust) => crate::daemon::tls::remove_ca_trust(),
        Some(Commands::Config { action, key, value }) => config::run_config(&action, key.as_deref(), value.as_deref()),
        Some(Commands::Map { path, codebase, check, deep }) => map::run_map(path, codebase, check, deep),
        Some(Commands::Gate { subcommand }) => gate::run_gate(&subcommand),
        Some(Commands::Scan { file, block, json, hook, install, uninstall, status, smart }) => {
            scan::run_scan(file, block, json, hook, install, uninstall, status, smart)
        }
        Some(Commands::Search { query, constraint, severity, since, model, limit }) => {
            search::run_search(query, constraint, severity, since, model, limit)
        }
        Some(Commands::Alert { command }) => alert::run_alert(command),
        Some(Commands::Diff { session_a, session_b, last }) => {
            diff::run_diff(session_a, session_b, last)
        }
        Some(Commands::Eval { report, baseline, compare }) => eval::run_eval(report, baseline, compare),
        Some(Commands::Refine { apply, dry_run }) => refine::run_refine(apply, dry_run),
        Some(Commands::Setup) => setup::run_setup(),
    }
}

/// Find rigor.yaml: use provided path or search up directory tree for rigor.yaml.
pub fn find_rigor_yaml(path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = path {
        if p.exists() {
            return Ok(p);
        }
        anyhow::bail!("File not found: {}", p.display());
    }

    // Search for rigor.yaml in current directory and parents
    let mut current = std::env::current_dir()?;
    loop {
        let candidate = current.join("rigor.yaml");
        if candidate.exists() {
            return Ok(candidate);
        }
        if !current.pop() {
            break;
        }
    }

    anyhow::bail!("No rigor.yaml found in current directory or any parent directory")
}
