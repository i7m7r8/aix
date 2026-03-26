use arti_client::{TorClient, TorClientConfig};
use arti_client::config::BridgesConfig;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, error};
use std::os::raw::c_int;

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct SniConfig {
    pub enabled: bool,
    pub custom_sni: String,
    pub bridge_line: String,
    pub last_updated: Option<String>,
}

#[derive(Default)]
pub struct TorManager {
    client: Arc<Mutex<Option<TorClient>>>,
    sni_config: Arc<Mutex<SniConfig>>,
    tun_fd: Arc<Mutex<Option<c_int>>>,
}

impl TorManager {
    pub fn new() -> Self { Self::default() }

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

    // JNI entry from Java
    #[no_mangle]
    pub extern "C" fn Java_com_i7m7r8_aix_TorVpnService_startTorWithTun(
        _env: jni::JNIEnv,
        _class: jni::objects::JClass,
        tun_fd: c_int,
    ) {
        info!("Received TUN FD from Android: {}", tun_fd);
        // TODO: spawn tokio runtime and route packets from this FD via Arti
        // For now we just store it
        // In production: use tokio::fs::File::from_raw_fd + packet loop
    }

    #[no_mangle]
    pub extern "C" fn Java_com_i7m7r8_aix_TorVpnService_stopTorNative(
        _env: jni::JNIEnv,
        _class: jni::objects::JClass,
    ) {
        // stop logic
    }
}
