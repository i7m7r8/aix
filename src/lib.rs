use arti_client::{TorClient, TorClientConfig};
use arti_client::config::{BridgesConfig, TransportConfig};
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;
use std::os::raw::c_int;
use jni::objects::{JClass, JString};
use jni::JNIEnv;

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

    pub async fn update_sni(&self, new_sni: SniConfig) -> Result<()> {
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
            bridges.set_enabled(true);
            bridges.set_bridges(vec![sni.bridge_line.clone()]);
            config_builder = config_builder.bridges(bridges);
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

// JNI implementation for VPN service
#[no_mangle]
pub extern "C" fn Java_com_i7m7r8_aix_TorVpnService_startTorWithTun(
    env: JNIEnv,
    _class: JClass,
    tun_fd: c_int,
    sni: JString,
    bridge: JString,
) {
    // Convert Java strings to Rust strings
    let sni_str: String = env.get_string(sni).unwrap().into();
    let bridge_str: String = env.get_string(bridge).unwrap().into();

    // Configure SNI
    let cfg = SniConfig {
        enabled: true,
        custom_sni: sni_str.clone(),
        bridge_line: bridge_str.clone(),
        last_updated: None,
    };
    let tm = TOR_MANAGER.clone();
    // Start the tunnel in a separate thread to avoid blocking the JNI call
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Err(e) = tm.update_sni(cfg).await {
                eprintln!("Failed to update SNI: {}", e);
                return;
            }
            if let Err(e) = tm.start_tor().await {
                eprintln!("Failed to start Tor: {}", e);
                return;
            }
            // TODO: Implement packet forwarding with tun_fd
            info!("TUN FD received: {} – packet forwarding not yet implemented", tun_fd);
            // For now, we just keep the thread alive (e.g., sleep forever)
            loop { tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await; }
        });
    });
}

#[no_mangle]
pub extern "C" fn Java_com_i7m7r8_aix_TorVpnService_stopTorNative(
    _env: JNIEnv,
    _class: JClass,
) {
    let tm = TOR_MANAGER.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            tm.stop_tor().await;
        });
    });
}
