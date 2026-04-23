//! `rigor serve` — persistent daemon mode.
//!
//! Unlike `rigor ground <cmd>`, which wraps a child process and dies when the
//! child exits, `rigor serve` runs rigor as a long-lived background daemon.
//! Any LLM tool (Claude Code, OpenCode, Cursor, your own scripts) can connect
//! to it by exporting `HTTPS_PROXY=http://127.0.0.1:8787`.
//!
//! Supported invocations:
//!   rigor serve                 -> run in foreground (Ctrl-C to stop)
//!   rigor serve --background    -> daemonize (fork, write PID to ~/.rigor/serve.pid)
//!   rigor serve --port 9090     -> alternate port
//!   rigor serve stop            -> kill the background daemon
//!
//! On SIGTERM / SIGINT we update the session registry entry with an `ended_at`
//! timestamp so `rigor sessions` shows a clean lifecycle.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::{Context, Result};

use crate::daemon::{self, build_router, DaemonState};
use crate::logging::session_registry::{self, SessionEntry};

/// Location of the `rigor serve` PID file. Distinct from `~/.rigor/daemon.pid`
/// (which is shared by ground/daemon modes) so that `rigor serve stop` only
/// ever kills a serve-mode daemon and never a `rigor ground` child.
pub fn serve_pid_file() -> Option<PathBuf> {
    Some(crate::paths::rigor_home().join("serve.pid"))
}

fn write_serve_pid(pid: u32) -> Result<()> {
    let path = serve_pid_file().context("cannot determine $HOME for serve PID file")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, format!("{}\n", pid))
        .with_context(|| format!("failed to write PID file at {}", path.display()))?;
    Ok(())
}

fn read_serve_pid() -> Option<i32> {
    let path = serve_pid_file()?;
    let txt = std::fs::read_to_string(&path).ok()?;
    txt.trim().parse::<i32>().ok()
}

fn remove_serve_pid() {
    if let Some(path) = serve_pid_file() {
        let _ = std::fs::remove_file(path);
    }
}

/// kill(pid, 0) liveness check — matches the one in daemon/mod.rs.
fn is_pid_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Entry point dispatched from `cli/mod.rs`.
pub fn run_serve(
    path: Option<PathBuf>,
    port: u16,
    background: bool,
    stop: bool,
    name: Option<String>,
    max_cost: Option<f64>,
) -> Result<()> {
    if stop {
        return stop_serve();
    }

    if background {
        return run_background(path, port, name);
    }

    run_foreground(path, port, name, max_cost)
}

/// Kill the running `rigor serve` background daemon, if any.
fn stop_serve() -> Result<()> {
    let Some(pid) = read_serve_pid() else {
        println!("rigor serve: no running daemon found (no PID file)");
        return Ok(());
    };

    if !is_pid_alive(pid) {
        println!(
            "rigor serve: stale PID file (pid {} not alive), cleaning up",
            pid
        );
        remove_serve_pid();
        return Ok(());
    }

    // SIGTERM first, give it a moment, then verify.
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }

    // Poll for exit up to ~2s.
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !is_pid_alive(pid) {
            remove_serve_pid();
            println!("rigor serve: stopped (pid {})", pid);
            return Ok(());
        }
    }

    // Escalate to SIGKILL if still alive.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
    remove_serve_pid();
    println!("rigor serve: force-killed (pid {})", pid);
    Ok(())
}

/// Fork into the background: the parent returns immediately after printing
/// connection instructions, the child re-execs itself as a foreground serve.
///
/// We deliberately avoid pulling in a daemonize crate. `fork + setsid +
/// redirect stdio + exec` is exactly what `daemon(3)` does on BSDs and is a
/// few lines of `libc` calls.
fn run_background(path: Option<PathBuf>, port: u16, name: Option<String>) -> Result<()> {
    // Bail early if a live daemon already claims the port.
    if let Some(pid) = read_serve_pid() {
        if is_pid_alive(pid) {
            anyhow::bail!(
                "rigor serve: already running (pid {}). Use `rigor serve stop` first.",
                pid
            );
        } else {
            remove_serve_pid();
        }
    }

    let exe = std::env::current_exe().context("cannot locate rigor binary for re-exec")?;

    // Log file for the background daemon. Users can `tail -f` this.
    let log_path = crate::paths::rigor_home().join("serve.log");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Double-fork so the grandchild is reparented to init and fully detached.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        anyhow::bail!("fork failed");
    }
    if pid > 0 {
        // Parent: wait briefly to catch obvious early failures, then exit.
        std::thread::sleep(std::time::Duration::from_millis(300));
        print_connection_banner(port, true, &log_path);
        return Ok(());
    }

    // Child: new session, detach from controlling tty.
    unsafe {
        libc::setsid();
    }

    // Redirect stdio to the log file so panics and eprintln! have somewhere to go.
    if let Ok(log_file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        use std::os::unix::io::AsRawFd;
        let fd = log_file.as_raw_fd();
        unsafe {
            libc::dup2(fd, 1);
            libc::dup2(fd, 2);
        }
        std::mem::forget(log_file);
    }

    // Re-exec ourselves as a foreground serve. This gives us a clean process
    // with no inherited runtimes / threads from the fork.
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("serve").arg("--port").arg(port.to_string());
    if let Some(p) = path {
        cmd.arg("--path").arg(p);
    }
    if let Some(n) = name {
        cmd.arg("--name").arg(n);
    }
    // Prevent the child from re-forking.
    cmd.env("RIGOR_SERVE_CHILD", "1");

    let err = cmd.exec_replace_self();
    // If exec returns, it failed.
    eprintln!("rigor serve: exec failed: {}", err);
    std::process::exit(1);
}

/// Small shim so the intent is obvious at the call site.
trait ExecReplaceSelf {
    fn exec_replace_self(&mut self) -> std::io::Error;
}
impl ExecReplaceSelf for std::process::Command {
    fn exec_replace_self(&mut self) -> std::io::Error {
        use std::os::unix::process::CommandExt;
        self.exec()
    }
}

/// Foreground mode: start the daemon, register the session, block until signal.
fn run_foreground(
    path: Option<PathBuf>,
    port: u16,
    session_name: Option<String>,
    max_cost: Option<f64>,
) -> Result<()> {
    // rigor serve is a global daemon — no project context at startup.
    // Constraints are loaded per-session when traffic arrives (via plugin headers).
    // If --path is given explicitly, load those constraints as defaults.
    let (mut state, constraint_count) = if let Some(ref p) = path {
        match crate::cli::find_rigor_yaml(Some(p.clone())) {
            Ok(yp) => {
                let (event_tx, _) = daemon::ws::create_event_channel();
                let s = DaemonState::load(yp, event_tx)?;
                let c = s.config.all_constraints().len();
                eprintln!("rigor serve: loaded {} constraints from {}", c, p.display());
                (s, c)
            }
            Err(_) => {
                let (event_tx, _) = daemon::ws::create_event_channel();
                let s = DaemonState::empty(event_tx)?;
                (s, 0)
            }
        }
    } else {
        let (event_tx, _) = daemon::ws::create_event_channel();
        let s = DaemonState::empty(event_tx)?;
        (s, 0)
    };
    state.max_cost_usd = max_cost;

    daemon::ws::set_quiet(false);
    daemon::ws::set_mitm_enabled(true);
    daemon::ws::set_transparent(false);
    daemon::ws::set_grounded_client(daemon::ws::GroundedClient::Unknown);

    // Register in the session registry so `rigor sessions` shows us.
    let session_id = uuid::Uuid::new_v4().to_string();
    let agent_str = "serve";
    let entry_name =
        session_name.unwrap_or_else(|| SessionEntry::auto_name(agent_str, &session_id));
    let entry = SessionEntry {
        id: session_id.clone(),
        name: entry_name.clone(),
        agent: agent_str.to_string(),
        started_at: chrono::Utc::now().to_rfc3339(),
        ended_at: None,
        pid: std::process::id(),
        constraints: constraint_count,
        config_path: path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(global)".to_string()),
        cwd: std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        requests: None,
        violations: None,
        total_tokens: None,
        exit_code: None,
    };
    if let Err(e) = session_registry::register_session(&entry) {
        eprintln!("rigor serve: warning: failed to register session: {}", e);
    }

    // Write both the shared daemon pid file (so hooks find us) and the
    // serve-specific pid file (so `rigor serve stop` finds us).
    if let Err(e) = daemon::write_pid_file() {
        eprintln!(
            "rigor serve: warning: could not write daemon pid file: {}",
            e
        );
    }
    if let Err(e) = write_serve_pid(std::process::id()) {
        eprintln!(
            "rigor serve: warning: could not write serve pid file: {}",
            e
        );
    }

    // Banner: only print to terminal in foreground mode. In background mode
    // the parent process already printed instructions before forking.
    let is_child = std::env::var_os("RIGOR_SERVE_CHILD").is_some();
    if !is_child {
        print_connection_banner(port, false, Path::new(""));
    }

    if constraint_count > 0 {
        eprintln!("rigor serve: {} constraints loaded", constraint_count);
    } else {
        eprintln!(
            "rigor serve: global mode — constraints loaded per-session from connecting projects"
        );
    }
    eprintln!(
        "rigor serve: session '{}' ({})",
        entry_name,
        &session_id[..8]
    );

    let shared = Arc::new(Mutex::new(state));

    // Build and run the HTTP router. We skip the TLS listener here —
    // `rigor serve` is purely the "point HTTPS_PROXY at us" flow, which uses
    // CONNECT + blind tunnel. Users who want TLS-terminated interception
    // should use `rigor ground` with the layer.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let shutdown_session_id = session_id.clone();

    rt.block_on(async move {
        let app = build_router(shared);
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("rigor serve: failed to bind {}: {}", addr, e);
                return;
            }
        };

        eprintln!("rigor serve: listening on http://127.0.0.1:{}", port);
        eprintln!("rigor serve: dashboard at http://127.0.0.1:{}", port);
        eprintln!("rigor serve: Ctrl+C to stop (or `rigor serve stop` if backgrounded)");

        let shutdown = shutdown_signal();

        let server = axum::serve(listener, app).with_graceful_shutdown(shutdown);
        if let Err(e) = server.await {
            eprintln!("rigor serve: server error: {}", e);
        }
    });

    // On graceful shutdown, mark the session as ended and drop PID files.
    eprintln!("rigor serve: shutting down cleanly");
    if let Err(e) = session_registry::update_session(&shutdown_session_id, |entry| {
        entry.ended_at = Some(chrono::Utc::now().to_rfc3339());
        entry.exit_code = Some(0);
    }) {
        eprintln!("rigor serve: warning: failed to update session: {}", e);
    }
    daemon::remove_pid_file();
    remove_serve_pid();

    Ok(())
}

/// Futures completes on SIGTERM or SIGINT. Uses tokio's signal primitives so
/// axum's graceful shutdown can hook in cleanly.
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};

    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut int = signal(SignalKind::interrupt()).expect("install SIGINT handler");

    tokio::select! {
        _ = term.recv() => {
            eprintln!("rigor serve: SIGTERM received");
        }
        _ = int.recv() => {
            eprintln!("rigor serve: SIGINT received");
        }
    }
}

/// Print the "how to connect" banner. `backgrounded` toggles the wording.
fn print_connection_banner(port: u16, backgrounded: bool, log_path: &Path) {
    let base = format!("http://127.0.0.1:{}", port);
    if backgrounded {
        println!("rigor serve: started in background");
        if !log_path.as_os_str().is_empty() {
            println!("rigor serve: logs at {}", log_path.display());
        }
    } else {
        println!("rigor serve: starting (foreground)");
    }
    println!();
    println!("Point any LLM tool at rigor:");
    println!("  export HTTPS_PROXY={}", base);
    println!("  export HTTP_PROXY={}", base);
    println!("  export ANTHROPIC_BASE_URL={}", base);
    println!("  export OPENAI_BASE_URL={}", base);
    println!();
    println!("Then run your tool (claude / opencode / etc) normally.");
    if backgrounded {
        println!("Stop the daemon with: rigor serve stop");
    }
    println!();
}
