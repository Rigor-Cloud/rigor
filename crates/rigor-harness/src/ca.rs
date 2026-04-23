use std::sync::Arc;
use anyhow::Result;
use rustls::ServerConfig;

pub struct TestCA {
    _placeholder: (),
}

impl TestCA {
    pub fn new() -> Result<Self> {
        todo!()
    }

    pub fn server_config_for_host(&self, _hostname: &str) -> Result<Arc<ServerConfig>> {
        todo!()
    }

    pub fn client_config(&self) -> rustls::ClientConfig {
        todo!()
    }

    pub fn reqwest_client(&self) -> reqwest::Client {
        todo!()
    }

    pub fn ca_cert_pem(&self) -> &str {
        todo!()
    }
}
