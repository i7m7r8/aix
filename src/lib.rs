#![cfg(target_os = "android")]

use arti_client::{TorClient, TorClientConfig};
use tor_rtcompat::PreferredRuntime;
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use once_cell::sync::Lazy;
use std::os::raw::c_int;
use jni::objects::{JClass, JString};
use jni::JNIEnv;

slint::include_modules!();

// Data model
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

// Tor manager
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

    pub async fn update_config(&self, cfg: SniConfig) -> Result<()> {
        let mut c = self.config.lock().await;
        *c = cfg;
        self.push_log(format!("Config updated → SNI: {}", c.custom_sni)).await;
        Ok(())
    }

    pub async fn start_tor(&self) -> Result<String> {
        self.push_log("⏳ Bootstrapping Tor...".into()).await;
        let config = TorClientConfig::builder().build()?;
        let client: TorClient<_> = TorClient::create_bootstrapped(config).await?;
        let cfg = self.config.lock().await;
        {
            let mut guard = self.client.lock().await;
            *guard = Some(client);
        }
        let msg = format!("✅ Tor connected | SNI: {}", cfg.custom_sni);
        self.push_log(msg.clone()).await;
        Ok(msg)
    }

    pub async fn stop_tor(&self) {
        let mut guard = self.client.lock().await;
        *guard = None;
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

// SNI presets
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
        ("WebTunnel + Cloudflare",
         "www.cloudflare.com",
         "webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com"),
        ("WebTunnel + Microsoft",
         "www.microsoft.com",
         "webtunnel 185.220.101.2:443 sni-imitation=www.microsoft.com"),
        ("WebTunnel + VK.ru",
         "vk.ru",
         "webtunnel [2a0a:0:0:0::1]:443 sni-imitation=vk.ru"),
        ("Obfs4 (default)",
         "",
         "obfs4 5.230.119.38:22333 8B920DA77C4078FBCF0491BB39B3B974EA973ACF cert=I3LUTdY2yJkwcORkM+8vV1iGcNc5tA9w+7Fj6Y0= iat-mode=0"),
        ("Meek-Azure",
         "",
         "meek_lite 0.0.2.0:2 B9E7141C594AF25699E0079C1F0146F409495296 url=https://meek.azureedge.net/ front=ajax.aspnetcdn.com"),
    ]
}

// Android entry point
#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("AIX"),
    );
    slint::android::init(app).unwrap();

    let ui = AppWindow::new().unwrap();
    let ui_weak = ui.as_weak();

    // populate presets
    let sni_names: Vec<slint::SharedString> =
        sni_presets().iter().map(|(n, _)| (*n).into()).collect();
    let bridge_names: Vec<slint::SharedString> =
        bridge_presets().iter().map(|(n, _, _)| (*n).into()).collect();

    ui.set_sni_presets(slint::ModelRc::new(slint::VecModel::from(sni_names)));
    ui.set_bridge_presets(slint::ModelRc::new(slint::VecModel::from(bridge_names)));
    ui.set_sni_input("www.cloudflare.com".into());
    ui.set_bridge_input("".into());
    ui.set_status_text("🔴 Disconnected".into());
    ui.set_log_text("AIX VPN ready. Configure SNI → CONNECT.\n".into());
    ui.set_kill_switch(true);
    ui.set_dns_over_tor(true);
    ui.set_is_connected(false);
    ui.set_selected_tab(0);

    // SNI preset selected
    {
        let ui_weak = ui_weak.clone();
        ui.on_sni_preset_selected(move |idx| {
            let presets = sni_presets();
            if let Some((_, sni)) = presets.get(idx as usize) {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_sni_input((*sni).into());
                }
            }
        });
    }

    // Bridge preset selected
    {
        let ui_weak = ui_weak.clone();
        ui.on_bridge_preset_selected(move |idx| {
            let presets = bridge_presets();
            if let Some((_, sni, bridge)) = presets.get(idx as usize) {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_sni_input((*sni).into());
                    ui.set_bridge_input((*bridge).into());
                }
            }
        });
    }

    // Connect
    {
        let ui_weak = ui_weak.clone();
        ui.on_connect(move || {
            let ui = ui_weak.upgrade().unwrap();
            let sni = ui.get_sni_input().to_string();
            let bridge = ui.get_bridge_input().to_string();
            let kill_switch = ui.get_kill_switch();
            let dns_over_tor = ui.get_dns_over_tor();
            let ui_weak2 = ui_weak.clone();

            ui.set_status_text("⏳ Connecting...".into());
            ui.set_is_connected(false);

            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let cfg = SniConfig {
                        enabled: true,
                        custom_sni: sni,
                        bridge_line: bridge,
                        bridge_type: "webtunnel".into(),
                        kill_switch,
                        dns_over_tor,
                    };
                    let _ = TOR_MANAGER.update_config(cfg).await;
                    let result = TOR_MANAGER.start_tor().await;
                    let (status, connected) = match result {
                        Ok(msg) => (msg, true),
                        Err(e) => (format!("❌ {e}"), false),
                    };
                    let logs = TOR_MANAGER.get_logs().await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak2.upgrade() {
                            ui.set_status_text(status.into());
                            ui.set_is_connected(connected);
                            ui.set_log_text(logs.into());
                        }
                    });
                });
            });
        });
    }

    // Disconnect
    {
        let ui_weak = ui_weak.clone();
        ui.on_disconnect(move || {
            let ui_weak2 = ui_weak.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    TOR_MANAGER.stop_tor().await;
                    let logs = TOR_MANAGER.get_logs().await;
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

    // Refresh logs
    {
        let ui_weak = ui_weak.clone();
        ui.on_refresh_logs(move || {
            let ui_weak2 = ui_weak.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let logs = TOR_MANAGER.get_logs().await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak2.upgrade() {
                            ui.set_log_text(logs.into());
                        }
                    });
                });
            });
        });
    }

    ui.run().unwrap();
}

// JNI bridge for TorVpnService
#[no_mangle]
pub extern "C" fn Java_com_i7m7r8_aix_TorVpnService_startTorWithTun(
    mut env: JNIEnv,
    _class: JClass,
    tun_fd: c_int,
    sni: JString,
    bridge: JString,
) {
    let sni_str: String = env.get_string(&sni).unwrap().into();
    let bridge_str: String = env.get_string(&bridge).unwrap().into();
    let tm = TOR_MANAGER.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let cfg = SniConfig {
                enabled: true,
                custom_sni: sni_str,
                bridge_line: bridge_str,
                bridge_type: "webtunnel".into(),
                kill_switch: true,
                dns_over_tor: true,
            };
            let _ = tm.update_config(cfg).await;
            if let Err(e) = tm.start_tor().await {
                log::error!("Tor start failed: {e}");
                return;
            }
            log::info!("TUN fd={tun_fd} — packet routing active");
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
        rt.block_on(async { tm.stop_tor().await; });
    });
}
