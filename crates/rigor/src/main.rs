use std::process;

fn main() {
    match rigor::cli::run_cli() {
        Ok(()) => process::exit(0),
        Err(e) => {
            // Check if we should fail closed (block on any error)
            let fail_closed = std::env::var("RIGOR_FAIL_CLOSED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false);

            if fail_closed {
                // Fail closed: exit code 2 blocks with stderr message only
                // Claude Code interprets exit code 2 as blocking error
                eprintln!("rigor: {:#}", e);
                process::exit(2);
            } else {
                // Fail open: return allow response with error metadata
                // This ensures Claude Code continues even if Rigor has issues
                let response = rigor::hook::HookResponse::error(format!("{:#}", e));
                if let Err(write_err) = response.write_stdout() {
                    // Last resort: stderr + exit 1 (non-blocking error)
                    eprintln!("rigor: Failed to write error response: {}", write_err);
                    eprintln!("rigor: Original error: {:#}", e);
                    process::exit(1);
                }
                process::exit(0);
            }
        }
    }
}
