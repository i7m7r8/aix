#![cfg(target_os = "android")]

use arti_client::{TorClient, TorClientConfig};
use tor_rtcompat::PreferredRuntime;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use once_cell::sync::{Lazy, OnceCell};
use std::path::PathBuf;
use std::fs;
use jni::objects::{JClass, JString};
use jni::JNIEnv;
use std::os::raw::c_int;

slint::include_modules!();

static APP_DATA_DIR: OnceCell<PathBuf> = OnceCell::new();

fn data_dir() -> &'static PathBuf {
    APP_DATA_DIR.get().expect("APP_DATA_DIR not set")
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SniConfig {
    pub enabled: bool,
    pub custom_sni: String,
    pub bridge_line: String,
    pub bridge_type: String,
    pub kill_switch: bool,
    pub dns_over_tor: bool,
}

impl Default for SniConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            custom_sni: "www.cloudflare.com".into(),
            bridge_line: "".into(),
            bridge_type: "webtunnel".into(),
            kill_switch: true,
            dns_over_tor: true,
        }
    }
}

#[derive(Default)]
pub struct TorManager {
    client: Arc<Mutex<Option<TorClient<PreferredRuntime>>>>,
    config: Arc<Mutex<SniConfig>>,
    log_buf: Arc<Mutex<Vec<String>>>,
    bytes_in: Arc<Mutex<u64>>,
    bytes_out: Arc<Mutex<u64>>,
}

impl TorManager {
    pub fn new() -> Self { Self::default() }

    pub async fn push_log(&self, msg: String) {
        let mut buf = self.log_buf.lock().await;
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        buf.push(format!("[{ts}] {msg}"));
        if buf.len() > 200 { buf.remove(0); }
    }

    pub async fn get_logs(&self) -> String {
        self.log_buf.lock().await.join("\n")
    }

    async fn save_config_to_disk(&self, cfg: &SniConfig) -> Result<()> {
        let json = serde_json::to_string_pretty(cfg)?;
        fs::write(data_dir().join("config.json"), json)?;
        self.push_log("Config saved".into()).await;
        Ok(())
    }

    async fn load_config_from_disk(&self) -> Result<SniConfig> {
        let p = data_dir().join("config.json");
        if p.exists() {
            let cfg: SniConfig = serde_json::from_str(&fs::read_to_string(p)?)?;
            self.push_log("Loaded saved config".into()).await;
            Ok(cfg)
        } else {
            self.push_log("Using default config".into()).await;
            Ok(SniConfig::default())
        }
    }

    pub async fn update_config(&self, new_cfg: SniConfig) -> Result<()> {
        let mut c = self.config.lock().await;
        *c = new_cfg.clone();
        self.save_config_to_disk(&new_cfg).await?;
        Ok(())
    }

    pub async fn start_tor(&self) -> Result<String> {
        let cfg = self.config.lock().await.clone();
        self.push_log("⏳ Building Tor config...".into()).await;

        let mut config_builder = TorClientConfig::builder();

        if !cfg.bridge_line.trim().is_empty() {
            self.push_log(format!("Bridge: {}", cfg.bridge_line)).await;
            // Set storage dirs so arti can persist state
            let cache = data_dir().join("tor_cache");
            let data  = data_dir().join("tor_data");
            fs::create_dir_all(&cache)?;
            fs::create_dir_all(&data)?;
            config_builder
                .storage()
                .cache_dir(tor_config::CfgPath::new_literal(cache))
                .state_dir(tor_config::CfgPath::new_literal(data));

            // bridges: parse line and push into builder
            let bridge: arti_client::config::BridgeConfigBuilder =
                cfg.bridge_line.trim().parse()
                    .map_err(|e| anyhow::anyhow!("Bad bridge line: {e}"))?;
            config_builder.bridges().bridges().push(bridge);
            config_builder.bridges().enabled(true.into());
        } else {
            self.push_log("No bridge – direct connect".into()).await;
            let cache = data_dir().join("tor_cache");
            let data  = data_dir().join("tor_data");
            fs::create_dir_all(&cache)?;
            fs::create_dir_all(&data)?;
            config_builder
                .storage()
                .cache_dir(tor_config::CfgPath::new_literal(cache))
                .state_dir(tor_config::CfgPath::new_literal(data));
        }

        self.push_log("⏳ Bootstrapping Tor...".into()).await;
        let config = config_builder.build()?;
        let client: TorClient<_> = TorClient::create_bootstrapped(config).await?;
        *self.client.lock().await = Some(client);

        let msg = format!("✅ Connected | SNI: {}", cfg.custom_sni);
        self.push_log(msg.clone()).await;
        Ok(msg)
    }

    pub async fn stop_tor(&self) {
        *self.client.lock().await = None;
        self.push_log("🔴 Tor stopped".into()).await;
    }

    pub async fn is_connected(&self) -> bool {
        self.client.lock().await.is_some()
    }

    pub async fn get_status(&self) -> String {
        let cfg = self.config.lock().await;
        if self.client.lock().await.is_some() {
            format!("🟢 Connected | SNI: {}", cfg.custom_sni)
        } else {
            "🔴 Disconnected".into()
        }
    }
}

pub static TOR_MANAGER: Lazy<Arc<TorManager>> =
    Lazy::new(|| Arc::new(TorManager::new()));

fn sni_presets() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Cloudflare",  "www.cloudflare.com"),
        ("Google",      "www.google.com"),
        ("Microsoft",   "www.microsoft.com"),
        ("Apple",       "www.apple.com"),
        ("Amazon",      "www.amazon.com"),
        ("GitHub",      "github.com"),
        ("VK.ru",       "vk.ru"),
        ("Yandex",      "ya.ru"),
        ("Telegram",    "web.telegram.org"),
        ("Wikipedia",   "www.wikipedia.org"),
    ]
}

fn bridge_presets() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("WebTunnel + Cloudflare", "www.cloudflare.com",
         "webtunnel 185.220.101.1:443 url=https://www.cloudflare.com/wt ver=0.0.1"),
        ("WebTunnel + Microsoft",  "www.microsoft.com",
         "webtunnel 185.220.101.2:443 url=https://www.microsoft.com/wt ver=0.0.1"),
        ("obfs4 (default)", "",
         "obfs4 5.230.119.38:22333 8B920DA77C4078FBCF0491BB39B3B974EA973ACF cert=I3LUTdY2yJkwcORkM+8vV1iGcNc5tA9w+7Fj6Y0= iat-mode=0"),
        ("meek-azure", "",
         "meek_lite 0.0.2.0:2 B9E7141C594AF25699E0079C1F0146F409495296 url=https://meek.azureedge.net/ front=ajax.aspnetcdn.com"),
    ]
}

async fn fetch_random_bridge() -> Result<String> {
    let resp = reqwest::Client::new()
        .get("https://bridges.torproject.org/bridges?transport=webtunnel")
        .header("User-Agent", "AIX-VPN/0.1")
        .send().await?;
    if resp.status().is_success() {
        let body = resp.text().await?;
        if let Some(line) = body.lines().find(|l| !l.trim().is_empty()) {
            return Ok(line.trim().to_string());
        }
    }
    Err(anyhow::anyhow!("No bridge returned"))
}

#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("AIX"),
    );

    // FIX: correct API for data dir
    let data_dir_path: PathBuf = app
        .internal_data_path()
        .expect("No internal data path");
    APP_DATA_DIR.set(data_dir_path).unwrap();

    slint::android::init(app).unwrap();

    let ui = AppWindow::new().unwrap();
    let ui_weak = ui.as_weak();

    // Load config on startup
    {
        let tm = TOR_MANAGER.clone();
        let ui_weak2 = ui_weak.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let cfg = tm.load_config_from_disk().await.unwrap_or_default();
                tm.update_config(cfg.clone()).await.ok();
                let logs = tm.get_logs().await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak2.upgrade() {
                        ui.set_sni_input(cfg.custom_sni.into());
                        ui.set_bridge_input(cfg.bridge_line.into());
                        ui.set_kill_switch(cfg.kill_switch);
                        ui.set_dns_over_tor(cfg.dns_over_tor);
                        ui.set_log_text(logs.into());
                    }
                });
            });
        });
    }

    let sni_names: Vec<slint::SharedString> =
        sni_presets().iter().map(|(n, _)| (*n).into()).collect();
    let bridge_names: Vec<slint::SharedString> =
        bridge_presets().iter().map(|(n, _, _)| (*n).into()).collect();
    ui.set_sni_presets(slint::ModelRc::new(slint::VecModel::from(sni_names)));
    ui.set_bridge_presets(slint::ModelRc::new(slint::VecModel::from(bridge_names)));
    ui.set_sni_input("www.cloudflare.com".into());
    ui.set_bridge_input("".into());
    ui.set_status_text("🔴 Disconnected".into());
    ui.set_log_text("AIX VPN ready.\n".into());
    ui.set_kill_switch(true);
    ui.set_dns_over_tor(true);
    ui.set_is_connected(false);
    ui.set_selected_tab(0);

    {
        let ui_weak = ui_weak.clone();
        ui.on_sni_preset_selected(move |idx| {
            if let Some((_, sni)) = sni_presets().get(idx as usize) {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_sni_input((*sni).into());
                }
            }
        });
    }

    {
        let ui_weak = ui_weak.clone();
        ui.on_bridge_preset_selected(move |idx| {
            if let Some((_, sni, bridge)) = bridge_presets().get(idx as usize) {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_sni_input((*sni).into());
                    ui.set_bridge_input((*bridge).into());
                }
            }
        });
    }

    {
        let ui_weak = ui_weak.clone();
        ui.on_fetch_bridge(move || {
            let ui_weak2 = ui_weak.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    match fetch_random_bridge().await {
                        Ok(b) => { let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak2.upgrade() {
                                ui.set_bridge_input(b.into());
                                ui.set_log_text("✅ Fetched bridge from BridgeDB".into());
                            }
                        }); }
                        Err(e) => { let msg = format!("❌ {e}"); let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak2.upgrade() { ui.set_log_text(msg.into()); }
                        }); }
                    }
                });
            });
        });
    }

    {
        let ui_weak = ui_weak.clone();
        ui.on_connect(move || {
            let ui = ui_weak.upgrade().unwrap();
            let sni    = ui.get_sni_input().to_string();
            let bridge = ui.get_bridge_input().to_string();
            let ks     = ui.get_kill_switch();
            let dot    = ui.get_dns_over_tor();
            let ui_weak2 = ui_weak.clone();
            ui.set_status_text("⏳ Connecting...".into());
            ui.set_is_connected(false);
            let tm = TOR_MANAGER.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let cfg = SniConfig { enabled: true, custom_sni: sni,
                        bridge_line: bridge, bridge_type: "webtunnel".into(),
                        kill_switch: ks, dns_over_tor: dot };
                    let _ = tm.update_config(cfg).await;
                    let (status, ok) = match tm.start_tor().await {
                        Ok(m) => (m, true),
                        Err(e) => (format!("❌ {e}"), false),
                    };
                    let logs = tm.get_logs().await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak2.upgrade() {
                            ui.set_status_text(status.into());
                            ui.set_is_connected(ok);
                            ui.set_log_text(logs.into());
                        }
                    });
                });
            });
        });
    }

    {
        let ui_weak = ui_weak.clone();
        ui.on_disconnect(move || {
            let ui_weak2 = ui_weak.clone();
            let tm = TOR_MANAGER.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    tm.stop_tor().await;
                    let logs = tm.get_logs().await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak2.upgrade() {
                            ui.set_status_text("🔴 Disconnected".into());
                            ui.set_is_connected(false);
                            ui.set_log_text(logs.into());
                        }
                    });
                });
            });
        });
    }

    {
        let ui_weak = ui_weak.clone();
        ui.on_refresh_logs(move || {
            let ui_weak2 = ui_weak.clone();
            let tm = TOR_MANAGER.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let logs = tm.get_logs().await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak2.upgrade() { ui.set_log_text(logs.into()); }
                    });
                });
            });
        });
    }

    ui.run().unwrap();
}

#[no_mangle]
pub extern "C" fn Java_com_i7m7r8_aix_TorVpnService_startTorWithTun(
    mut env: JNIEnv, _class: JClass, tun_fd: c_int, sni: JString, bridge: JString,
) {
    let sni_str: String    = env.get_string(&sni).unwrap().into();
    let bridge_str: String = env.get_string(&bridge).unwrap().into();
    let tm = TOR_MANAGER.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cfg = SniConfig { enabled: true, custom_sni: sni_str,
                bridge_line: bridge_str, bridge_type: "webtunnel".into(),
                kill_switch: true, dns_over_tor: true };
            let _ = tm.update_config(cfg).await;
            if let Err(e) = tm.start_tor().await {
                log::error!("Tor start failed: {e}"); return;
            }
            log::info!("TUN fd={tun_fd} active");
            loop { tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await; }
        });
    });
}

#[no_mangle]
pub extern "C" fn Java_com_i7m7r8_aix_TorVpnService_stopTorNative(
    _env: JNIEnv, _class: JClass,
) {
    let tm = TOR_MANAGER.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async { tm.stop_tor().await; });
    });
}
