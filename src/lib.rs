use arti_client::{TorClient, TorClientConfig};
use arti_client::config::BridgesConfig;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, error};

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct SniConfig {
    pub enabled: bool,
    pub custom_sni: String,        // e.g. "www.cloudflare.com"
    pub bridge_line: String,       // Full bridge with sni-imitation=...
    pub last_updated: Option<String>,
}

#[derive(Default)]
pub struct TorManager {
    client: Arc<Mutex<Option<TorClient>>>,
    sni_config: Arc<Mutex<SniConfig>>,
}

impl TorManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn update_sni(&self, mut new_sni: SniConfig) -> Result<()> {
        new_sni.last_updated = Some(chrono::Utc::now().to_rfc3339());
        let mut cfg = self.sni_config.lock().await;
        *cfg = new_sni;
        info!("Custom SNI updated → {}", cfg.custom_sni);
        Ok(())
    }

    pub async fn start_tor(&self) -> Result<String> {
        let mut config_builder = TorClientConfig::builder();

        let sni = self.sni_config.lock().await;
        if sni.enabled && !sni.bridge_line.trim().is_empty() {
            let mut bridges = BridgesConfig::default();
            bridges.set_bridges(vec![sni.bridge_line.clone()])?;
            config_builder.set_bridges(bridges);
            info!("Loading bridge with custom SNI: {}", sni.custom_sni);
        }

        let config = config_builder.build()?;
        let tor_client = TorClient::create(config).await?;

        let mut guard = self.client.lock().await;
        *guard = Some(tor_client);

        Ok(format!("✅ Tor started with SNI: {}", sni.custom_sni))
    }

    pub async fn stop_tor(&self) {
        let mut guard = self.client.lock().await;
        *guard = None;
        info!("Tor stopped");
    }

    pub async fn get_status(&self) -> String {
        let guard = self.client.lock().await;
        let sni = self.sni_config.lock().await;
        if guard.is_some() {
            format!("🟢 Connected | SNI: {}", if sni.enabled { &sni.custom_sni } else { "default" })
        } else {
            "🔴 Disconnected".to_string()
        }
    }
}
