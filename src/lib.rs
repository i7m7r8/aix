use arti_client::{TorClient, TorClientConfig};
use arti_client::config::BridgesConfig;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, error};
use std::os::raw::c_int;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tun_rs::AsyncDevice;

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
    routing_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
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
        if let Some(task) = self.routing_task.lock().await.take() {
            task.abort();
        }
        info!("Tor stopped");
    }

    // Called from Java when TUN FD is ready
    #[no_mangle]
    pub extern "C" fn Java_com_i7m7r8_aix_TorVpnService_startTorWithTun(
        _env: jni::JNIEnv,
        _class: jni::objects::JClass,
        tun_fd: c_int,
    ) {
        info!("Received TUN FD: {}", tun_fd);
        let manager = crate::TOR_MANAGER.clone(); // we'll expose it globally

        tokio::spawn(async move {
            if let Err(e) = start_routing(tun_fd, manager).await {
                error!("Routing failed: {}", e);
            }
        });
    }

    #[no_mangle]
    pub extern "C" fn Java_com_i7m7r8_aix_TorVpnService_stopTorNative(
        _env: jni::JNIEnv,
        _class: jni::objects::JClass,
    ) {
        // stop logic handled in stop_tor
    }
}

// Basic routing skeleton (read from TUN → forward via Arti, write back)
// This is simplified — real version needs packet parsing + SOCKS or stream per connection
async fn start_routing(tun_fd: c_int, manager: Arc<TorManager>) -> Result<()> {
    // Use tun-rs AsyncDevice from raw fd (platform specific on Android)
    // For simplicity, we use tokio::fs::File from raw fd + basic loop
    unsafe {
        let file = std::fs::File::from_raw_fd(tun_fd);
        let mut device = tokio::fs::File::from_std(file);  // async wrapper

        let mut buf = vec![0u8; 2048];
        loop {
            let n = device.read(&mut buf).await?;
            if n == 0 { break; }

            // TODO: Parse IP packet, extract dest, create Arti stream or use SOCKS
            // For now: log and echo back (demo)
            info!("Received {} bytes from TUN", n);
            // In real impl: forward via tor_client.connect() or SOCKS proxy
            device.write_all(&buf[..n]).await?;
        }
    }
    Ok(())
}

// Global for JNI access
use once_cell::sync::Lazy;
pub static TOR_MANAGER: Lazy<Arc<TorManager>> = Lazy::new(|| Arc::new(TorManager::new()));
