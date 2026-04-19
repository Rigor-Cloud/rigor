use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;

use crate::{daemon, info_println};

/// Check if a command exists in PATH.
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Patch a SIP-protected macOS binary for DYLD_INSERT_LIBRARIES injection.
/// mirrord pattern: copy the binary, strip code signature, re-sign ad-hoc.
fn patch_sip_binary(binary_path: &str) -> Result<PathBuf> {
    let src = std::path::Path::new(binary_path);
    if !src.exists() {
        anyhow::bail!("binary not found: {}", binary_path);
    }

    let bin_name = src
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let patch_dir = std::path::PathBuf::from("/tmp/rigor-patched");
    std::fs::create_dir_all(&patch_dir)?;

    let patched = patch_dir.join(&bin_name);

    // Only re-patch if the patched version doesn't exist or is older
    let needs_patch = if patched.exists() {
        let src_meta = std::fs::metadata(src)?;
        let dst_meta = std::fs::metadata(&patched)?;
        src_meta.modified()? > dst_meta.modified()?
    } else {
        true
    };

    if needs_patch {
        // Copy the binary
        std::fs::copy(src, &patched)?;

        // Create an entitlements plist that allows DYLD_INSERT_LIBRARIES.
        // The key: com.apple.security.cs.disable-library-validation tells macOS
        // to accept injected dylibs even with hardened runtime enabled.
        // This is the mirrord approach — keep the binary structurally intact,
        // just add the entitlement and re-sign ad-hoc.
        let entitlements_path = patch_dir.join("entitlements.plist");
        std::fs::write(&entitlements_path, r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.cs.allow-jit</key>
    <true/>
    <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
    <true/>
    <key>com.apple.security.cs.disable-library-validation</key>
    <true/>
    <key>com.apple.security.cs.allow-dyld-environment-variables</key>
    <true/>
</dict>
</plist>
"#)?;

        // Re-sign ad-hoc with the new entitlements.
        // --force overwrites existing signature, -s - means ad-hoc,
        // --entitlements adds our plist.
        let output = Command::new("codesign")
            .args([
                "--force",
                "--sign", "-",
                "--entitlements", &entitlements_path.to_string_lossy(),
                &patched.to_string_lossy(),
            ])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                info_println!("rigor: re-signed with DYLD entitlements");
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                eprintln!("rigor: codesign warning: {}", stderr.trim());
            }
            Err(e) => {
                eprintln!("rigor: codesign failed: {}", e);
            }
        }

        let _ = Command::new("chmod")
            .args(["+x", &patched.to_string_lossy()])
            .output();
    }

    Ok(patched)
}

/// Interception mode used to redirect AI tool traffic through rigor.
#[derive(Debug, Clone)]
enum InterceptionMode {
    /// HTTP/HTTPS proxy env vars — works with Node.js, Python, most HTTP clients
    HttpProxy,
    /// LD_PRELOAD / DYLD_INSERT_LIBRARIES — hooks libc connect()
    LdPreload(PathBuf),
}

/// Find the rigor-layer shared library.
fn find_layer_lib() -> Option<PathBuf> {
    let candidates = [
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf())),
        Some(PathBuf::from("target/release")),
        Some(PathBuf::from("target/debug")),
        Some(PathBuf::from("layer/target/release")),
        Some(PathBuf::from("layer/target/debug")),
    ];

    let lib_name = if cfg!(target_os = "macos") {
        "librigor_layer.dylib"
    } else {
        "librigor_layer.so"
    };

    for dir in candidates.into_iter().flatten() {
        let path = dir.join(lib_name);
        if path.exists() {
            return Some(std::fs::canonicalize(&path).unwrap_or(path));
        }
    }

    None
}

/// Apply interception environment variables to the command.
fn apply_interception(cmd: &mut Command, mode: &InterceptionMode, port: u16) {
    let base_url = format!("http://127.0.0.1:{}", port);

    match mode {
        InterceptionMode::HttpProxy => {
            // Set base URL vars — these redirect LLM API calls to rigor daemon
            // without affecting other network traffic (OAuth, etc.)
            cmd.env("ANTHROPIC_BASE_URL", &base_url);
            cmd.env("OPENAI_BASE_URL", &base_url);

            // For Vertex AI: set the endpoint override
            cmd.env("CLOUD_ML_API_ENDPOINT", &format!("127.0.0.1:{}", port));

            info_println!("rigor: proxy mode (ANTHROPIC_BASE_URL={})", base_url);
        }
        InterceptionMode::LdPreload(layer_path) => {
            let layer_str = layer_path.to_string_lossy().to_string();

            if cfg!(target_os = "macos") {
                let existing = std::env::var("DYLD_INSERT_LIBRARIES").unwrap_or_default();
                let new_val = if existing.is_empty() {
                    layer_str.clone()
                } else {
                    format!("{}:{}", layer_str, existing)
                };
                cmd.env("DYLD_INSERT_LIBRARIES", &new_val);

                // macOS: bypass cert validation for redirected connections
                cmd.env("NODE_TLS_REJECT_UNAUTHORIZED", "0");
            } else {
                let existing = std::env::var("LD_PRELOAD").unwrap_or_default();
                let new_val = if existing.is_empty() {
                    layer_str.clone()
                } else {
                    format!("{}:{}", layer_str, existing)
                };
                cmd.env("LD_PRELOAD", &new_val);
                cmd.env("NODE_TLS_REJECT_UNAUTHORIZED", "0");
            }

            // Disable TLS cert verification so the process accepts our self-signed cert
            // Bun (Claude Code) and Node.js both respect this
            cmd.env("NODE_TLS_REJECT_UNAUTHORIZED", "0");

            info_println!("rigor: LD_PRELOAD mode ({})", layer_str);
        }
    }

    cmd.env("RIGOR_DAEMON_PORT", port.to_string());
    if !crate::daemon::ws::is_quiet() {
        cmd.env("RIGOR_LAYER_DEBUG", "1");
    }

    // In transparent mode, the layer hooks connect() to redirect outbound :443
    // directly to rigor's TLS port. We DO NOT set HTTPS_PROXY env vars because
    // some clients (Claude Code) disable OAuth when they detect a proxy.
    if crate::daemon::ws::is_transparent() {
        cmd.env("RIGOR_TRANSPARENT", "1");
    } else {
        // Default: HTTPS_PROXY env vars catch Bun/Go runtimes that bypass libc DNS.
        cmd.env("HTTPS_PROXY", &base_url);
        cmd.env("HTTP_PROXY", &base_url);
        cmd.env("https_proxy", &base_url);
        cmd.env("http_proxy", &base_url);
        cmd.env("NO_PROXY", "localhost,127.0.0.1");
        cmd.env("no_proxy", "localhost,127.0.0.1");
    }

    if matches!(mode, InterceptionMode::HttpProxy) {
        cmd.env("ANTHROPIC_BASE_URL", &base_url);
        cmd.env("OPENAI_BASE_URL", &base_url);
        cmd.env("CLOUD_ML_API_ENDPOINT", &format!("127.0.0.1:{}", port));
    }
}

/// Run `rigor ground <command>` — epistemically ground an AI process.
///
/// Starts the rigor daemon, then spawns the target command with interception
/// configured so all LLM API calls flow through rigor.
///
/// Interception strategy (tries in order):
/// 1. LD_PRELOAD (if layer built) — hooks connect() at libc level
/// 2. HTTP_PROXY + ANTHROPIC_BASE_URL — env var based redirection
pub fn run_ground(path: Option<PathBuf>, port: u16, quiet: bool, mitm: bool, transparent: bool, command: Vec<String>) -> Result<()> {
    use std::os::unix::io::{AsRawFd, FromRawFd};

    // Open the rigor ground log file. Always overwrite — fresh log per run.
    // This is where ALL daemon output goes so the terminal stays clean.
    let log_path = std::path::PathBuf::from("/tmp/rigor-ground.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)?;
    let log_fd = log_file.as_raw_fd();

    // Save original stderr/stdout/stdin so we can pass them to the child process.
    // Without this, Claude's TUI would write to our log file instead of the terminal.
    let saved_stderr = unsafe { libc::dup(2) };
    let saved_stdout = unsafe { libc::dup(1) };
    let saved_stdin = unsafe { libc::dup(0) };
    if saved_stderr < 0 || saved_stdout < 0 || saved_stdin < 0 {
        anyhow::bail!("Failed to save original std fds");
    }

    // Redirect rigor's own stderr to the log file. The daemon thread, info_println!,
    // emit_log, eprintln! and panics all write to fd 2 — they'll all go to the log now.
    if unsafe { libc::dup2(log_fd, 2) } < 0 {
        anyhow::bail!("Failed to dup2 stderr to log file");
    }
    // Keep the file alive — drop just closes our handle, fd 2 still references it.
    std::mem::forget(log_file);

    // Print the log path to the ORIGINAL terminal so the user knows where to look.
    let banner = format!(
        "rigor: logs at {} (tail -f to follow)\n",
        log_path.display()
    );
    unsafe {
        libc::write(saved_stderr, banner.as_ptr() as *const _, banner.len());
    }

    // Apply quiet flag globally — emit_log and other helpers check this
    crate::daemon::ws::set_quiet(quiet);

    // Apply MITM flag globally — should_mitm_target checks this
    crate::daemon::ws::set_mitm_enabled(mitm);

    // Apply transparent flag globally — layer reads RIGOR_TRANSPARENT env var
    crate::daemon::ws::set_transparent(transparent);

    if transparent {
        info_println!("rigor: transparent mode — layer redirects all :443 to daemon (no HTTPS_PROXY)");
    }
    if mitm {
        info_println!("rigor: MITM mode enabled — LLM endpoints will be inspected (may break OAuth/cert pinning)");
    } else if !transparent {
        info_println!("rigor: blind tunnel mode (default) — all CONNECT tunnels preserve end-to-end TLS");
        info_println!("rigor: pass --mitm to enable LLM body inspection and constraint injection");
    }

    if command.is_empty() {
        anyhow::bail!(
            "Usage: rigor ground <command> [args...]\n\
             Example: rigor ground claude --dangerously-skip-permissions"
        );
    }

    let yaml_path = crate::cli::find_rigor_yaml(path)?;
    let (event_tx, _event_rx) = daemon::ws::create_event_channel();
    let state = daemon::DaemonState::load(yaml_path.clone(), event_tx)?;

    let constraint_count = state.config.all_constraints().len();
    info_println!(
        "rigor: grounding with {} constraints from {}",
        constraint_count,
        yaml_path.display()
    );

    let event_tx_for_server = {
        let st = state.event_tx.clone();
        st
    };

    let shared = std::sync::Arc::new(std::sync::Mutex::new(state));
    let shared_for_server = shared.clone();

    // Determine interception mode
    // frida-gum hooks getaddrinfo (DNS), not connect — works on macOS and Linux
    let mode = match find_layer_lib() {
        Some(layer_path) => InterceptionMode::LdPreload(layer_path),
        None => InterceptionMode::HttpProxy,
    };

    info_println!("rigor: interception: {:?}", mode);

    let uses_ldpreload = matches!(mode, InterceptionMode::LdPreload(_));

    // Advertise ourselves to the hooks. `rigor ground` is the usual way to
    // start a daemon (via `rigor-personal`), but before this call only
    // `rigor daemon` (which calls start_daemon() in daemon/mod.rs) wrote
    // the pid file — so gate + Stop hooks thought no daemon was running
    // even while rigor-personal had one serving requests. The kill(pid, 0)
    // liveness check in daemon_alive() handles stale files on next check.
    if let Err(e) = daemon::write_pid_file() {
        eprintln!("rigor ground: warning — could not write pid file: {}", e);
    }

    // Start daemon in background thread (HTTP + optional HTTPS for LD_PRELOAD)
    let server_handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let app = daemon::build_router(shared_for_server);

            // HTTP listener (dashboard + plaintext proxy)
            let http_app = app.clone();
            let http_handle = tokio::spawn(async move {
                let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
                info_println!("rigor daemon: http://127.0.0.1:{} (dashboard)", port);
                let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
                axum::serve(listener, http_app).await.unwrap();
            });

            if uses_ldpreload {
                // HTTPS listener for LD_PRELOAD intercepted traffic
                // TLS port: default 443, configurable via RIGOR_DAEMON_TLS_PORT for non-root usage
                let tls_port: u16 = std::env::var("RIGOR_DAEMON_TLS_PORT")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(443);
                let tls_app = app;
                let tls_event_tx = event_tx_for_server.clone();
                let tls_handle = tokio::spawn(async move {
                    let tls_config = match daemon::tls::generate_tls_config(daemon::MITM_HOSTS) {
                        Ok(c) => c,
                        Err(e) => {
                            daemon::ws::emit_log(&tls_event_tx, "error", "tls",
                                format!("TLS setup failed: {} — LD_PRELOAD won't work", e));
                            return;
                        }
                    };

                    let tls_acceptor = tokio_rustls::TlsAcceptor::from(
                        std::sync::Arc::new(tls_config),
                    );
                    let listener = match tokio::net::TcpListener::bind(
                        format!("127.0.0.1:{}", tls_port),
                    ).await {
                        Ok(l) => l,
                        Err(e) => {
                            daemon::ws::emit_log(&tls_event_tx, "error", "tls",
                                format!("Failed to bind TLS port {}: {}", tls_port, e));
                            return;
                        }
                    };

                    daemon::ws::emit_log(&tls_event_tx, "info", "tls",
                        format!("TLS listener ready on 127.0.0.1:{}", tls_port));

                    loop {
                        let (stream, addr) = match listener.accept().await {
                            Ok(conn) => conn,
                            Err(e) => {
                                daemon::ws::emit_log(&tls_event_tx, "warn", "net",
                                    format!("TCP accept error: {}", e));
                                continue;
                            }
                        };

                        daemon::ws::emit_log(&tls_event_tx, "info", "net",
                            format!("TCP accept from {}", addr));

                        let acceptor = tls_acceptor.clone();
                        let app = tls_app.clone();
                        let conn_event_tx = tls_event_tx.clone();

                        tokio::spawn(async move {
                            // Peek the TLS ClientHello to extract SNI before deciding
                            // whether to MITM or blind-tunnel. This is the mirrord pattern:
                            // the layer redirects all :443 here, and we route based on SNI.
                            let mut stream = stream;
                            let (peeked, sni_opt) = match daemon::sni::peek_client_hello(&mut stream).await {
                                Ok(v) => v,
                                Err(e) => {
                                    // Connection reset / aborted before TLS started — usually a probe
                                    // or connection pool init that the client immediately closed.
                                    let msg = e.to_string();
                                    let level = if msg.contains("reset") || msg.contains("aborted")
                                        || msg.contains("unexpected end") {
                                        "debug"
                                    } else {
                                        "warn"
                                    };
                                    daemon::ws::emit_log(&conn_event_tx, level, "tls",
                                        format!("SNI peek aborted for {}: {}", addr, e));
                                    return;
                                }
                            };

                            let sni_host = sni_opt.clone().unwrap_or_else(|| "<no-sni>".to_string());
                            daemon::ws::emit_log(&conn_event_tx, "info", "tls",
                                format!("SNI peeked from {}: {}", addr, sni_host));

                            // Decide MITM or blind tunnel based on SNI hostname
                            let should_mitm = sni_opt.as_ref()
                                .map(|h| daemon::should_mitm_target(&format!("{}:443", h)))
                                .unwrap_or(false);

                            if !should_mitm {
                                // BLIND TUNNEL: open new TCP to the real upstream and pipe bytes,
                                // including the buffered ClientHello we already read.
                                let upstream_target = sni_opt.as_deref()
                                    .map(|h| format!("{}:443", h))
                                    .unwrap_or_else(|| "127.0.0.1:443".to_string());

                                daemon::ws::emit_log(&conn_event_tx, "info", "proxy",
                                    format!("Blind tunneling SNI={} → {}", sni_host, upstream_target));

                                let mut upstream = match tokio::net::TcpStream::connect(&upstream_target).await {
                                    Ok(u) => u,
                                    Err(e) => {
                                        daemon::ws::emit_log(&conn_event_tx, "error", "net",
                                            format!("Blind tunnel upstream connect failed for {}: {}", upstream_target, e));
                                        return;
                                    }
                                };

                                // Forward the buffered ClientHello bytes first
                                use tokio::io::AsyncWriteExt;
                                if let Err(e) = upstream.write_all(&peeked).await {
                                    daemon::ws::emit_log(&conn_event_tx, "warn", "proxy",
                                        format!("Failed to forward ClientHello to {}: {}", upstream_target, e));
                                    return;
                                }

                                // Now pipe bytes both ways
                                let mut prepended = stream;
                                match tokio::io::copy_bidirectional(&mut prepended, &mut upstream).await {
                                    Ok((from_client, from_upstream)) => {
                                        daemon::ws::emit_log(&conn_event_tx, "info", "proxy",
                                            format!("Blind tunnel closed: {} ({}B out, {}B in)",
                                                upstream_target, from_client, from_upstream));
                                    }
                                    Err(e) => {
                                        let msg = e.to_string();
                                        if !msg.contains("connection reset") && !msg.contains("broken pipe") {
                                            daemon::ws::emit_log(&conn_event_tx, "warn", "proxy",
                                                format!("Blind tunnel copy error for {}: {}", upstream_target, e));
                                        }
                                    }
                                }
                                return;
                            }

                            // MITM PATH: replay buffered bytes through the TLS acceptor
                            let prepended = daemon::sni::PrependedStream::new(peeked, stream);
                            match acceptor.accept(prepended).await {
                                Ok(tls_stream) => {
                                    daemon::ws::emit_log(&conn_event_tx, "info", "tls",
                                        format!("MITM TLS handshake OK from {} (SNI={})", addr, sni_host));
                                    let io = hyper_util::rt::TokioIo::new(tls_stream);
                                    let tower_service = app.clone();
                                    let req_event_tx = conn_event_tx.clone();
                                    let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                        let mut router = tower_service.clone();
                                        let log_tx = req_event_tx.clone();
                                        let method = req.method().to_string();
                                        let path = req.uri().path().to_string();
                                        async move {
                                            daemon::ws::emit_log(&log_tx, "info", "proxy",
                                                format!("HTTP request received: {} {}", method, path));
                                            use tower::Service;
                                            let (parts, body) = req.into_parts();
                                            let body = axum::body::Body::new(body);
                                            let req = hyper::Request::from_parts(parts, body);
                                            router.call(req).await.map_err(|e| {
                                                std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                                            })
                                        }
                                    });
                                    if let Err(e) = hyper_util::server::conn::auto::Builder::new(
                                        hyper_util::rt::TokioExecutor::new(),
                                    )
                                    .serve_connection(io, service)
                                    .await
                                    {
                                        let msg = e.to_string();
                                        if !msg.contains("connection closed") && !msg.contains("broken pipe") {
                                            daemon::ws::emit_log(&conn_event_tx, "warn", "proxy",
                                                format!("HTTP service error from {}: {}", addr, msg));
                                        }
                                    }
                                }
                                Err(e) => {
                                    daemon::ws::emit_log(&conn_event_tx, "error", "tls",
                                        format!("TLS handshake FAILED from {}: {}", addr, e));
                                }
                            }
                        });
                    }
                });

                // Wait for BOTH tasks. join! lets the HTTP server keep running
                // even if the TLS listener fails (e.g., can't bind port 443
                // without sudo). select! would cancel the survivor on first exit.
                let _ = tokio::join!(http_handle, tls_handle);
            } else {
                http_handle.await.unwrap();
            }
        });
    });

    // Give daemon time to start
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Open dashboard
    let _ = open::that(&format!("http://127.0.0.1:{}", port));

    // Spawn the target command
    // If the first arg is an existing binary, exec it directly.
    // Otherwise, spawn via the user's shell so aliases/functions work.
    let program = &command[0];
    let is_binary = std::path::Path::new(program).exists()
        || which_exists(program);

    let mut cmd;
    let actual_mode;

    if is_binary {
        // Direct binary — can use LD_PRELOAD
        // On macOS, check if binary has hardened runtime (strips DYLD_INSERT_LIBRARIES)
        // If so, patch it (copy + strip signature + re-sign ad-hoc) like mirrord does
        let effective_program = if cfg!(target_os = "macos") && matches!(mode, InterceptionMode::LdPreload(_)) {
            match patch_sip_binary(program) {
                Ok(patched) => {
                    info_println!("rigor: patched hardened runtime binary → {}", patched.display());
                    patched.to_string_lossy().to_string()
                }
                Err(e) => {
                    eprintln!("rigor: binary patch failed ({}), using original", e);
                    program.clone()
                }
            }
        } else {
            program.clone()
        };

        cmd = Command::new(&effective_program);
        cmd.args(&command[1..]);
        actual_mode = mode.clone();
    } else {
        // Shell alias/function — spawn via user's shell
        let full_command = command.join(" ");
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

        if cfg!(target_os = "macos") && matches!(mode, InterceptionMode::LdPreload(_)) {
            // macOS system shells are arm64e — our arm64 dylib can't load into them.
            // Use proxy mode for the shell, LD_PRELOAD will apply to child processes
            // that are arm64 (like user-installed node/claude binaries).
            info_println!("rigor: shell alias → proxy mode (macOS system shells are arm64e)");
            info_println!("rigor: tip: use direct binary path for LD_PRELOAD: rigor ground /path/to/claude ...");
            cmd = Command::new(&shell);
            cmd.arg("-i").arg("-c").arg(&full_command);
            actual_mode = InterceptionMode::HttpProxy;
        } else {
            cmd = Command::new(&shell);
            cmd.arg("-i").arg("-c").arg(&full_command);
            actual_mode = mode.clone();
        }
    };

    apply_interception(&mut cmd, &actual_mode, port);

    // Wire the child process's stdio to the ORIGINAL terminal fds, not our
    // log-file-redirected fds. Without this, Claude's TUI would write to the log file.
    cmd.stdin(unsafe { std::process::Stdio::from_raw_fd(saved_stdin) });
    cmd.stdout(unsafe { std::process::Stdio::from_raw_fd(saved_stdout) });
    cmd.stderr(unsafe { std::process::Stdio::from_raw_fd(saved_stderr) });

    info_println!("rigor: spawning: {}", command.join(" "));
    info_println!("");

    let status = cmd.status();

    match status {
        Ok(s) => info_println!("\nrigor: process exited with {}", s),
        Err(e) => eprintln!("\nrigor: failed to spawn: {}", e),
    }

    drop(server_handle);
    Ok(())
}
