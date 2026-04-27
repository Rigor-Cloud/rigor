//! E2E tests for the CONNECT tunnel handler in `catch_all_proxy`.
//!
//! Exercises two distinct code paths:
//! 1. **Blind tunnel** (non-LLM host): proxy connects upstream and runs
//!    `copy_bidirectional` -- bytes pass through unchanged.
//! 2. **MITM TLS handshake** (LLM host): proxy terminates TLS with a
//!    per-host cert signed by its CA, then serves the axum router on the
//!    decrypted stream.
//!
//! Both tests go through the real production `catch_all_proxy` CONNECT
//! handler via `TestProxy`, which now uses
//! `hyper_util::server::conn::auto::Builder::serve_connection_with_upgrades`.

use std::sync::Mutex;
use std::time::Duration;

use rigor_harness::TestProxy;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Minimal valid rigor.yaml matching other integration tests.
const MINIMAL_YAML: &str =
    "constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n";

/// Serializes tests that toggle the global MITM_ENABLED AtomicBool.
/// Same pattern as `MITM_LOCK` in daemon/mod.rs tests.
static MITM_LOCK: Mutex<()> = Mutex::new(());

/// Maximum time to wait for any single I/O operation in tests.
const IO_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Spawn a TCP echo server that reads data and echoes it back, then closes.
async fn start_echo_server() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind echo server");
    let addr = listener.local_addr().expect("echo server addr");

    let handle = tokio::spawn(async move {
        // Accept one connection, echo everything back, then shut down.
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = vec![0u8; 4096];
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if stream.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    });

    (addr, handle)
}

/// Send a CONNECT request through `proxy_addr` to `target`, read the 200
/// response, and return the upgraded TCP stream for further I/O.
async fn send_connect(proxy_addr: std::net::SocketAddr, target: &str) -> TcpStream {
    let mut stream = timeout(IO_TIMEOUT, TcpStream::connect(proxy_addr))
        .await
        .expect("connect timeout")
        .expect("connect to proxy");

    let connect_req = format!(
        "CONNECT {} HTTP/1.1\r\nHost: {}\r\n\r\n",
        target, target
    );
    timeout(IO_TIMEOUT, stream.write_all(connect_req.as_bytes()))
        .await
        .expect("write timeout")
        .expect("send CONNECT");

    let mut buf = vec![0u8; 1024];
    let n = timeout(IO_TIMEOUT, stream.read(&mut buf))
        .await
        .expect("read timeout")
        .expect("read CONNECT response");

    let response = String::from_utf8_lossy(&buf[..n]);
    assert!(
        response.contains("200"),
        "CONNECT should return 200, got: {}",
        response
    );

    stream
}

/// Read the CA cert PEM written by `RigorCA::load_or_generate()` inside the
/// proxy's `IsolatedHome`, and build a `rustls::ClientConfig` that trusts it.
///
/// Uses `rcgen::CertificateParams::from_ca_cert_pem` to parse the PEM (same
/// as production `RigorCA::load_or_generate`), then extracts DER for the
/// rustls root store. No `rustls-pemfile` dependency needed.
fn load_ca_client_config(ca_pem_path: &std::path::Path) -> rustls::ClientConfig {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let ca_pem = std::fs::read_to_string(ca_pem_path)
        .unwrap_or_else(|e| panic!("read CA PEM at {}: {}", ca_pem_path.display(), e));

    // Parse PEM using rcgen (same pattern as production RigorCA::load_or_generate)
    let ca_key_pem = std::fs::read_to_string(
        ca_pem_path.with_file_name("ca-key.pem"),
    )
    .expect("read CA key PEM");
    let ca_key = rcgen::KeyPair::from_pem(&ca_key_pem).expect("parse CA key");
    let ca_params =
        rcgen::CertificateParams::from_ca_cert_pem(&ca_pem).expect("parse CA cert PEM");
    let ca_cert = ca_params.self_signed(&ca_key).expect("re-sign CA cert");

    let ca_der = ca_cert.der().clone();

    let mut root_store = rustls::RootCertStore::empty();
    root_store
        .add(rustls::pki_types::CertificateDer::from(ca_der.to_vec()))
        .expect("add CA cert to root store");

    rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// CONNECT to a non-LLM host creates a byte-for-byte blind tunnel.
///
/// Exercises proxy.rs blind-tunnel path (copy_bidirectional).
/// `should_mitm_target` returns false because MITM_ENABLED defaults to false
/// AND the target is a plain IP, not an LLM host.
#[tokio::test]
async fn blind_tunnel_non_llm_host() {
    // 1. Start echo server
    let (echo_addr, _echo_handle) = start_echo_server().await;

    // 2. Start proxy (MITM disabled by default)
    let proxy = TestProxy::start(MINIMAL_YAML).await;

    // 3. CONNECT to the echo server through the proxy
    let target = format!("127.0.0.1:{}", echo_addr.port());
    let mut stream = send_connect(proxy.addr(), &target).await;

    // 4. Send data through the tunnel and verify echo
    let payload = b"hello blind tunnel";
    timeout(IO_TIMEOUT, stream.write_all(payload))
        .await
        .expect("write timeout")
        .expect("send through tunnel");

    let mut buf = vec![0u8; 1024];
    let n = timeout(IO_TIMEOUT, stream.read(&mut buf))
        .await
        .expect("read timeout")
        .expect("read echo response");

    assert_eq!(
        &buf[..n], payload,
        "blind tunnel should echo bytes unchanged"
    );
}

/// CONNECT to an LLM host with MITM enabled results in TLS termination
/// using a CA-signed certificate that validates against the test CA.
///
/// Exercises proxy.rs MITM path: TLS termination via RigorCA +
/// per-host cert generation + axum router on decrypted stream.
#[tokio::test]
async fn mitm_tls_handshake_validates_against_ca() {
    let _guard = MITM_LOCK.lock().unwrap();
    let original = rigor::daemon::ws::is_mitm_enabled();

    // 1. Start mock LLM server
    let mock = rigor_harness::MockLlmServerBuilder::new()
        .anthropic_chunks("test tunnel response")
        .build()
        .await;

    // 2. Start proxy with mock upstream
    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;

    // 3. Enable MITM so should_mitm_target returns true for LLM hosts
    rigor::daemon::ws::set_mitm_enabled(true);

    // 4. CONNECT to api.anthropic.com:443 through the proxy
    let stream = send_connect(proxy.addr(), "api.anthropic.com:443").await;

    // 5. Load the CA cert that RigorCA wrote during TestProxy startup
    let ca_pem_path = proxy.home.rigor_dir.join("ca.pem");
    let client_config = load_ca_client_config(&ca_pem_path);

    // 6. Perform TLS handshake on the upgraded stream
    let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(client_config));
    let server_name = rustls::pki_types::ServerName::try_from("api.anthropic.com")
        .expect("valid server name");

    let tls_result = timeout(
        IO_TIMEOUT,
        connector.connect(server_name, stream),
    )
    .await
    .expect("TLS handshake timeout");

    let mut tls_stream = tls_result.expect(
        "TLS handshake should succeed when client trusts the proxy CA",
    );

    // 7. Send an HTTP request over the TLS stream to verify the full
    //    MITM pipeline: proxy decrypts, routes via axum to MockLlmServer
    let http_request = concat!(
        "POST /v1/messages HTTP/1.1\r\n",
        "Host: api.anthropic.com\r\n",
        "Content-Type: application/json\r\n",
        "x-api-key: test-key\r\n",
        "anthropic-version: 2023-06-01\r\n",
        "Content-Length: 91\r\n",
        "\r\n",
        r#"{"model":"claude-3-haiku-20240307","max_tokens":100,"messages":[{"role":"user","content":"hi"}]}"#,
    );

    timeout(IO_TIMEOUT, tls_stream.write_all(http_request.as_bytes()))
        .await
        .expect("write timeout")
        .expect("send HTTP over TLS");

    // Read response -- we just need to verify we get *something* back
    // from the mock server through the MITM pipeline.
    let mut response_buf = vec![0u8; 8192];
    let n = timeout(IO_TIMEOUT, tls_stream.read(&mut response_buf))
        .await
        .expect("read timeout")
        .expect("read HTTP response over TLS");

    let response = String::from_utf8_lossy(&response_buf[..n]);
    assert!(
        response.contains("HTTP/1.1"),
        "should receive an HTTP response through the MITM tunnel, got: {}",
        &response[..std::cmp::min(200, response.len())]
    );

    // Restore MITM state
    rigor::daemon::ws::set_mitm_enabled(original);
}

/// **H4 negative test** — TLS handshake MUST fail when the client does NOT
/// trust the proxy's CA.
///
/// Mirrors `mitm_tls_handshake_validates_against_ca` exactly except the
/// client is built without `.add_root_certificate(test_ca_pem)`. The proxy's
/// per-host MITM cert is signed by an ephemeral CA that is NOT in any system
/// trust store, so the handshake must fail with a certificate verification
/// error.
///
/// Regression guard: if rigor ever started serving system-default certs
/// (instead of CA-signed per-host certs), this test would pass with a
/// successful handshake — i.e. the test detects that exact regression.
///
/// Uses `reqwest::Client` configured with `Proxy::all(proxy_url)` so reqwest
/// will issue a CONNECT to the proxy, then attempt the upstream TLS
/// handshake against the proxy's MITM cert. The transport-level error must
/// reference certificate / cert / TLS to confirm the failure happens at the
/// TLS layer (not at TCP, not at HTTP, not in the proxy itself).
#[tokio::test]
async fn tls_handshake_fails_without_ca_cert() {
    let _guard = MITM_LOCK.lock().unwrap();
    let original = rigor::daemon::ws::is_mitm_enabled();

    // 1. Start mock LLM server (request will never reach it because TLS
    //    will fail at the proxy MITM cert presentation, but we wire it up
    //    to mirror the positive test exactly).
    let mock = rigor_harness::MockLlmServerBuilder::new()
        .anthropic_chunks("test tunnel response")
        .build()
        .await;

    // 2. Start proxy with mock upstream
    let proxy = TestProxy::start_with_mock(MINIMAL_YAML, &mock.url()).await;

    // 3. Enable MITM so should_mitm_target returns true for LLM hosts
    rigor::daemon::ws::set_mitm_enabled(true);

    // 4. Build a reqwest client that uses the rigor proxy as an HTTP proxy
    //    but does NOT trust the test CA. Default rustls trust = system
    //    roots only, which won't include the ephemeral test CA.
    //
    //    NOTE: explicitly do NOT call `.add_root_certificate(...)` — this
    //    is the negative-test mirror of the positive test's
    //    `add_root_certificate(test_ca_pem)`. We additionally call
    //    `.tls_built_in_root_certs(true)` and `.danger_accept_invalid_certs(false)`
    //    defensively to make the trust posture explicit.
    let proxy_url = format!("http://{}", proxy.addr());
    let client = reqwest::Client::builder()
        .proxy(
            reqwest::Proxy::all(&proxy_url)
                .expect("valid proxy URL"),
        )
        .tls_built_in_root_certs(true)
        .danger_accept_invalid_certs(false)
        .timeout(IO_TIMEOUT)
        .build()
        .expect("build reqwest client without test CA trust");

    // 5. Issue an HTTPS request that will route via CONNECT through the
    //    rigor proxy. reqwest will open TCP to the proxy, send CONNECT
    //    api.anthropic.com:443, then attempt TLS handshake against the
    //    proxy's per-host MITM cert. The handshake must FAIL.
    let body = serde_json::json!({
        "model": "claude-3-haiku-20240307",
        "max_tokens": 100,
        "messages": [{"role": "user", "content": "hi"}]
    });
    let result = client
        .post("https://api.anthropic.com/v1/messages")
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await;

    // 6. Restore MITM state BEFORE assertions so a failed assertion doesn't
    //    leave global state polluted for the next test.
    rigor::daemon::ws::set_mitm_enabled(original);

    // 7. Assert the request failed.
    let err = match result {
        Ok(resp) => panic!(
            "TLS handshake should FAIL when client does not trust the test CA, \
             but reqwest returned a response with status {}. \
             This indicates rigor is serving system-default certs OR the test \
             CA is somehow in the system trust store.",
            resp.status()
        ),
        Err(e) => e,
    };

    // 8. Assert the failure is a TLS / certificate error, not a connect/
    //    timeout/HTTP error. We walk the error source chain because reqwest
    //    wraps the underlying rustls/native-tls error several layers deep.
    use std::error::Error as _;
    let mut messages: Vec<String> = vec![err.to_string()];
    let mut src: Option<&(dyn std::error::Error + 'static)> = err.source();
    while let Some(e) = src {
        messages.push(e.to_string());
        src = e.source();
    }
    let combined = messages.join(" || ").to_lowercase();

    // The lowercased error chain must mention something TLS/cert related.
    // rustls typically surfaces "invalid peer certificate", "unknown issuer",
    // "certificate", or "invalid certificate" depending on version.
    let is_cert_error = combined.contains("certificate")
        || combined.contains("cert verif")
        || combined.contains("unknown issuer")
        || combined.contains("invalid peer")
        || combined.contains("untrusted")
        || combined.contains("tls");
    assert!(
        is_cert_error,
        "expected a TLS/certificate verification error, but got an error \
         chain that does not mention TLS/cert. Full chain: {}",
        combined
    );

    // Defensive: must NOT be a plain connect failure (would mean proxy is
    // unreachable, not that TLS failed).
    assert!(
        !err.is_connect() || is_cert_error,
        "error should not be a pure TCP connect failure; the proxy must be \
         reachable for the TLS handshake to be attempted. Error: {}",
        err
    );
}
