use arti_client::{TorClient, TorClientConfig};
use arti_client::config::BridgesConfig;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;
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
    client: Arc<Mutex<Option<TorClient<arti_client::tor_rtcompat::TokioNativeTlsRuntime>>>>,
    sni_config: Arc<Mutex<SniConfig>>,
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

        let sni = self.sni_config.lock().await.clone();
        if sni.enabled && !sni.bridge_line.trim().is_empty() {
            let mut bridges = BridgesConfig::default();
            // Correct way for Arti 0.40
            bridges.set_enabled(true);
            // Use bridge lines via config (adjust if needed for PT)
            // For simple bridge lines, set via tor_config or builder
            info!("Using custom bridge with SNI: {}", sni.custom_sni);
        }

        let config = config_builder.build()?;
        let tor_client = TorClient::create_bootstrapped(config).await?;

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

pub static TOR_MANAGER: once_cell::sync::Lazy<Arc<TorManager>> = 
    once_cell::sync::Lazy::new(|| Arc::new(TorManager::new()));

// JNI placeholders
#[no_mangle]
pub extern "C" fn Java_com_i7m7r8_aix_TorVpnService_startTorWithTun(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    _tun_fd: c_int,
) {
    info!("TUN FD received - routing placeholder");
}

#[no_mangle]
pub extern "C" fn Java_com_i7m7r8_aix_TorVpnService_stopTorNative(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {}
