use std::net::SocketAddr;
use tokio::sync::oneshot;

pub struct MockLlmServerBuilder {
    _placeholder: (),
}

pub struct MockLlmServer {
    addr: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl MockLlmServerBuilder {
    pub fn new() -> Self {
        todo!()
    }

    pub fn anthropic_chunks(self, _text: &str) -> Self {
        todo!()
    }

    pub fn openai_chunks(self, _text: &str) -> Self {
        todo!()
    }

    pub fn raw_chunks(self, _chunks: Vec<String>) -> Self {
        todo!()
    }

    pub fn route(self, _path: &str) -> Self {
        todo!()
    }

    pub async fn build(self) -> MockLlmServer {
        todo!()
    }
}

impl MockLlmServer {
    pub async fn start(_chunks: Vec<String>) -> Self {
        todo!()
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }
}

impl Drop for MockLlmServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}
