use crate::env_lock::ENV_LOCK;
use crate::home::IsolatedHome;
use rigor::daemon::ws::{DaemonEvent, EventSender};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, oneshot};
use tower::Service;

/// A test proxy wrapping the production `build_router` + `DaemonState` on an ephemeral port.
///
/// Uses `IsolatedHome` so `DaemonState::load` never touches real `~/.rigor/`.
/// Sets `RIGOR_HOME` (not `HOME`) to the isolated `.rigor/` directory.
/// Shuts down cleanly on Drop via a oneshot channel.
pub struct TestProxy {
    addr: SocketAddr,
    pub home: IsolatedHome,
    /// Broadcast sender for daemon events. Tests can call `subscribe()` to
    /// observe `DaemonEvent::PiiDetected`, `DaemonEvent::Violation`,
    /// `DaemonEvent::Decision`, etc. emitted by the proxy hot path.
    event_tx: EventSender,
    shutdown_tx: Option<oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl TestProxy {
    /// Start a TestProxy with a rigor.yaml config string.
    ///
    /// Creates an `IsolatedHome`, writes `rigor_yaml` into it, loads `DaemonState`
    /// with HOME pointed at the isolated directory, and binds the production router
    /// to an ephemeral port.
    ///
    /// RIGOR_HOME isolation: Uses `tokio::task::spawn_blocking` to temporarily set
    /// RIGOR_HOME for the `DaemonState::load` call (which internally calls
    /// `rigor_home()` via `RigorCA::load_or_generate` and `judge_config`).
    pub async fn start(rigor_yaml: &str) -> Self {
        let home = IsolatedHome::new();
        let yaml_path = home.write_rigor_yaml(rigor_yaml);
        let rigor_home_str = home.rigor_dir_str();

        let (event_tx, _event_rx) = rigor::daemon::ws::create_event_channel();

        let state = {
            let yaml_path = yaml_path.clone();
            let event_tx = event_tx.clone();
            let rigor_home_str = rigor_home_str.clone();
            tokio::task::spawn_blocking(move || {
                let _guard = ENV_LOCK.lock().unwrap();
                let original_rigor_home = std::env::var("RIGOR_HOME").ok();
                unsafe { std::env::set_var("RIGOR_HOME", &rigor_home_str) };
                let result = rigor::daemon::DaemonState::load(yaml_path, event_tx);
                match original_rigor_home {
                    Some(h) => unsafe { std::env::set_var("RIGOR_HOME", h) },
                    None => unsafe { std::env::remove_var("RIGOR_HOME") },
                }
                drop(_guard);
                result.expect("DaemonState::load failed in TestProxy")
            })
            .await
            .expect("spawn_blocking join failed")
        };

        let shared: rigor::daemon::SharedState = Arc::new(Mutex::new(state));
        let app = rigor::daemon::build_router(shared);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind TestProxy to ephemeral port");
        let addr = listener.local_addr().expect("get TestProxy local addr");

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let handle = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let (stream, _addr) = match result {
                            Ok(conn) => conn,
                            Err(_) => continue,
                        };
                        let app = app.clone();
                        tokio::spawn(async move {
                            let io = hyper_util::rt::TokioIo::new(stream);
                            let service = hyper::service::service_fn(
                                move |req: hyper::Request<hyper::body::Incoming>| {
                                    let mut app = app.clone();
                                    async move {
                                        let (parts, incoming) = req.into_parts();
                                        let body = axum::body::Body::new(incoming);
                                        let req = axum::http::Request::from_parts(parts, body);
                                        let resp = app.call(req).await.unwrap();
                                        Ok::<_, std::convert::Infallible>(resp)
                                    }
                                },
                            );
                            let _ = hyper_util::server::conn::auto::Builder::new(
                                hyper_util::rt::TokioExecutor::new(),
                            )
                            .serve_connection_with_upgrades(io, service)
                            .await;
                        });
                    }
                    _ = &mut shutdown_rx => { break; }
                }
            }
        });

        Self {
            addr,
            home,
            event_tx,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    /// Start a TestProxy pointed at a MockLlmServer upstream.
    ///
    /// Sets `RIGOR_TARGET_API` to `mock_url` before creating `DaemonState`,
    /// ensuring the proxy forwards to the mock instead of hitting a real API.
    pub async fn start_with_mock(rigor_yaml: &str, mock_url: &str) -> Self {
        let home = IsolatedHome::new();
        let yaml_path = home.write_rigor_yaml(rigor_yaml);
        let rigor_home_str = home.rigor_dir_str();
        let mock_url = mock_url.to_string();

        let (event_tx, _event_rx) = rigor::daemon::ws::create_event_channel();

        let state = {
            let yaml_path = yaml_path.clone();
            let event_tx = event_tx.clone();
            let rigor_home_str = rigor_home_str.clone();
            let mock_url = mock_url.clone();
            tokio::task::spawn_blocking(move || {
                let _guard = ENV_LOCK.lock().unwrap();
                let original_rigor_home = std::env::var("RIGOR_HOME").ok();
                let original_target = std::env::var("RIGOR_TARGET_API").ok();
                unsafe {
                    std::env::set_var("RIGOR_HOME", &rigor_home_str);
                    std::env::set_var("RIGOR_TARGET_API", &mock_url);
                };
                let result = rigor::daemon::DaemonState::load(yaml_path, event_tx);
                match original_rigor_home {
                    Some(h) => unsafe { std::env::set_var("RIGOR_HOME", h) },
                    None => unsafe { std::env::remove_var("RIGOR_HOME") },
                }
                match original_target {
                    Some(t) => unsafe { std::env::set_var("RIGOR_TARGET_API", t) },
                    None => unsafe { std::env::remove_var("RIGOR_TARGET_API") },
                }
                drop(_guard);
                result.expect("DaemonState::load failed in TestProxy::start_with_mock")
            })
            .await
            .expect("spawn_blocking join failed")
        };

        let shared: rigor::daemon::SharedState = Arc::new(Mutex::new(state));
        let app = rigor::daemon::build_router(shared);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind TestProxy to ephemeral port");
        let addr = listener.local_addr().expect("get TestProxy local addr");

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let handle = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let (stream, _addr) = match result {
                            Ok(conn) => conn,
                            Err(_) => continue,
                        };
                        let app = app.clone();
                        tokio::spawn(async move {
                            let io = hyper_util::rt::TokioIo::new(stream);
                            let service = hyper::service::service_fn(
                                move |req: hyper::Request<hyper::body::Incoming>| {
                                    let mut app = app.clone();
                                    async move {
                                        let (parts, incoming) = req.into_parts();
                                        let body = axum::body::Body::new(incoming);
                                        let req = axum::http::Request::from_parts(parts, body);
                                        let resp = app.call(req).await.unwrap();
                                        Ok::<_, std::convert::Infallible>(resp)
                                    }
                                },
                            );
                            let _ = hyper_util::server::conn::auto::Builder::new(
                                hyper_util::rt::TokioExecutor::new(),
                            )
                            .serve_connection_with_upgrades(io, service)
                            .await;
                        });
                    }
                    _ = &mut shutdown_rx => { break; }
                }
            }
        });

        Self {
            addr,
            home,
            event_tx,
            shutdown_tx: Some(shutdown_tx),
            _handle: handle,
        }
    }

    /// The base URL of the proxy (e.g. `http://127.0.0.1:12345`).
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The socket address the proxy is listening on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Subscribe to the daemon event broadcast channel.
    ///
    /// Tests use this to observe `DaemonEvent::PiiDetected`,
    /// `DaemonEvent::Violation`, `DaemonEvent::Decision`, and other events
    /// emitted by the proxy hot path. Subscribe BEFORE making the request to
    /// avoid missing events.
    pub fn subscribe(&self) -> broadcast::Receiver<DaemonEvent> {
        self.event_tx.subscribe()
    }
}

impl Drop for TestProxy {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid rigor.yaml (ConstraintsSection is a struct, not a list).
    const MINIMAL_YAML: &str = "constraints:\n  beliefs: []\n  justifications: []\n  defeaters: []\n";

    #[tokio::test]
    async fn test_proxy_starts_on_ephemeral_port() {
        let proxy = TestProxy::start(MINIMAL_YAML).await;
        assert_ne!(proxy.addr().port(), 0);
    }

    #[tokio::test]
    async fn test_proxy_url_format() {
        let proxy = TestProxy::start(MINIMAL_YAML).await;
        let url = proxy.url();
        assert!(url.starts_with("http://127.0.0.1:"), "url should be http://127.0.0.1:PORT, got: {}", url);
    }
}
