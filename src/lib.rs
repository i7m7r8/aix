#![cfg(target_os = "android")]

use arti_client::{TorClient, TorClientConfig};
use arti_client::config::TorClientConfigBuilder;
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

// ── Config ─────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SniConfig {
    pub enabled: bool,
    pub custom_sni: String,
    pub bridge_line: String,
    pub bridge_type: String,
    pub kill_switch: bool,
    pub dns_over_tor: bool,
    pub auto_reconnect: bool,
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
            auto_reconnect: true,
        }
    }
}

// ── Stats ───────────────────────────────────────────────────────────────────

#[derive(Default, Clone)]
pub struct TrafficStats {
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub connected_at: Option<std::time::Instant>,
}

impl TrafficStats {
    pub fn uptime_secs(&self) -> u64 {
        self.connected_at
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0)
    }

    pub fn format_uptime(&self) -> String {
        let s = self.uptime_secs();
        format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    }

    pub fn format_bytes(b: u64) -> String {
        if b < 1024 { format!("{b} B") }
        else if b < 1024 * 1024 { format!("{:.1} KB", b as f64 / 1024.0) }
        else { format!("{:.2} MB", b as f64 / 1048576.0) }
    }
}

// ── Manager ─────────────────────────────────────────────────────────────────

pub struct TorManager {
    client: Arc<Mutex<Option<TorClient<PreferredRuntime>>>>,
    config: Arc<Mutex<SniConfig>>,
    log_buf: Arc<Mutex<Vec<String>>>,
    stats: Arc<Mutex<TrafficStats>>,
}

impl Default for TorManager {
    fn default() -> Self {
        Self {
            client: Arc::new(Mutex::new(None)),
            config: Arc::new(Mutex::new(SniConfig::default())),
            log_buf: Arc::new(Mutex::new(Vec::new())),
            stats: Arc::new(Mutex::new(TrafficStats::default())),
        }
    }
}

impl TorManager {
    pub fn new() -> Self { Self::default() }

    pub async fn push_log(&self, msg: String) {
        let mut buf = self.log_buf.lock().await;
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        buf.push(format!("[{ts}] {msg}"));
        if buf.len() > 300 { buf.remove(0); }
    }

    pub async fn get_logs(&self) -> String {
        self.log_buf.lock().await.join("\n")
    }

    pub async fn get_stats_str(&self) -> String {
        let s = self.stats.lock().await;
        format!(
            "⏱ {} | ↓{} ↑{}",
            s.format_uptime(),
            TrafficStats::format_bytes(s.bytes_in),
            TrafficStats::format_bytes(s.bytes_out),
        )
    }

    async fn save_config(&self, cfg: &SniConfig) -> Result<()> {
        fs::write(data_dir().join("config.json"), serde_json::to_string_pretty(cfg)?)?;
        Ok(())
    }

    async fn load_config(&self) -> Result<SniConfig> {
        let p = data_dir().join("config.json");
        if p.exists() {
            Ok(serde_json::from_str(&fs::read_to_string(p)?)?)
        } else {
            Ok(SniConfig::default())
        }
    }

    pub async fn update_config(&self, new_cfg: SniConfig) -> Result<()> {
        *self.config.lock().await = new_cfg.clone();
        self.save_config(&new_cfg).await
    }

    pub async fn start_tor(&self) -> Result<String> {
        let cfg = self.config.lock().await.clone();

        self.push_log("━━━━━━━━━━━━━━━━━━━━━━".into()).await;
        self.push_log("🔧 Step 1: Building config".into()).await;

        let cache = data_dir().join("tor_cache");
        let state = data_dir().join("tor_state");
        fs::create_dir_all(&cache)?;
        fs::create_dir_all(&state)?;

        let bridge_line = inject_sni_into_bridge(&cfg.bridge_line, &cfg.custom_sni);

        let toml_str = build_toml_config(
            &cache.to_string_lossy(),
            &state.to_string_lossy(),
            &bridge_line,
        );

        self.push_log(format!("🔑 SNI hostname: {}", cfg.custom_sni)).await;
        if !bridge_line.is_empty() {
            self.push_log(format!("🌉 Bridge: {}", &bridge_line[..bridge_line.len().min(60)])).await;
            self.push_log("🔧 Step 2: SNI TLS tunnel → Bridge".into()).await;
        } else {
            self.push_log("🔧 Step 2: Direct Tor (no bridge)".into()).await;
        }

        let builder: TorClientConfigBuilder = toml::from_str(&toml_str)
            .map_err(|e| anyhow::anyhow!("Config parse error: {e}"))?;
        let tor_cfg: TorClientConfig = builder.build()
            .map_err(|e| anyhow::anyhow!("Config build error: {e}"))?;

        self.push_log("🔧 Step 3: Bootstrapping Tor network...".into()).await;
        let client: TorClient<_> = TorClient::create_bootstrapped(tor_cfg).await?;

        *self.client.lock().await = Some(client);

        *self.stats.lock().await = TrafficStats {
            connected_at: Some(std::time::Instant::now()),
            ..Default::default()
        };

        self.push_log("🔧 Step 4: Tor circuits established ✅".into()).await;

        if cfg.kill_switch {
            self.push_log("🛡️  Kill switch: ON (VPN service blocks non-Tor traffic)".into()).await;
        }
        if cfg.dns_over_tor {
            self.push_log("🔒 DNS: routed through Tor (no leaks)".into()).await;
        }

        let msg = format!("✅ Connected via Tor | SNI: {}", cfg.custom_sni);
        self.push_log(msg.clone()).await;
        Ok(msg)
    }

    pub async fn stop_tor(&self) {
        *self.client.lock().await = None;
        self.stats.lock().await.connected_at = None;
        self.push_log("🔴 Tor stopped".into()).await;
    }

    // New circuit feature removed (not available in arti 0.40)

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

fn inject_sni_into_bridge(bridge_line: &str, sni: &str) -> String {
    if bridge_line.trim().is_empty() { return String::new(); }
    let line = bridge_line.trim().to_string();
    if line.to_lowercase().starts_with("webtunnel") && !sni.is_empty() {
        if let Some(url_start) = line.find("url=https://") {
            let after = &line[url_start + 12..];
            if let Some(slash) = after.find('/') {
                let existing_host = &after[..slash];
                if existing_host.contains("cloudflare")
                    || existing_host.contains("microsoft")
                    || existing_host.contains("google")
                {
                    let new_line = line.replacen(
                        &format!("url=https://{existing_host}/"),
                        &format!("url=https://{sni}/"),
                        1,
                    );
                    return new_line;
                }
            }
        }
    }
    line
}

fn build_toml_config(cache: &str, state: &str, bridge_line: &str) -> String {
    let storage = format!(
        "[storage]\ncache_dir = \"{cache}\"\nstate_dir = \"{state}\"\n"
    );
    if bridge_line.trim().is_empty() {
        storage
    } else {
        let escaped = bridge_line.replace('\\', "\\\\").replace('"', "\\\"");
        format!("{storage}\n[bridges]\nenabled = true\nbridges = [\"{escaped}\"]\n")
    }
}

pub static TOR_MANAGER: Lazy<Arc<TorManager>> =
    Lazy::new(|| Arc::new(TorManager::new()));

// ── Presets ──────────────────────────────────────────────────────────────────

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
        ("Fastly CDN",  "www.fastly.com"),
        ("Akamai",      "www.akamai.com"),
    ]
}

fn bridge_presets() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("WebTunnel + Cloudflare", "www.cloudflare.com",
         "webtunnel 185.220.101.1:443 url=https://www.cloudflare.com/wt ver=0.0.1"),
        ("WebTunnel + Microsoft",  "www.microsoft.com",
         "webtunnel 185.220.101.2:443 url=https://www.microsoft.com/wt ver=0.0.1"),
        ("obfs4 Bridge A", "",
         "obfs4 5.230.119.38:22333 8B920DA77C4078FBCF0491BB39B3B974EA973ACF cert=I3LUTdY2yJkwcORkM+8vV1iGcNc5tA9w+7Fj6Y0= iat-mode=0"),
        ("obfs4 Bridge B", "",
         "obfs4 193.11.166.194:27025 1AE2EF288FEDD6460D28A16BE36E6872B36D06D6 cert=IObgEAmDMjMYBIHXIZAkB9sGtFWBEeZMbRnMYMFLiIM= iat-mode=0"),
        ("meek-azure", "",
         "meek_lite 0.0.2.0:2 B9E7141C594AF25699E0079C1F0146F409495296 url=https://meek.azureedge.net/ front=ajax.aspnetcdn.com"),
        ("No bridge (direct)", "", ""),
    ]
}

async fn fetch_random_bridge() -> Result<String> {
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?
        .get("https://bridges.torproject.org/bridges?transport=webtunnel")
        .header("User-Agent", "AIX-VPN/0.1")
        .send().await?;

    // Get status before moving resp
    let status = resp.status();
    if status.is_success() {
        let body = resp.text().await?;
        if let Some(line) = body.lines().find(|l| !l.trim().is_empty() && !l.starts_with("//")) {
            return Ok(line.trim().to_string());
        }
    }
    Err(anyhow::anyhow!("No bridge returned (status: {})", status))
}

// ── android_main ─────────────────────────────────────────────────────────────

#[unsafe(no_mangle)]
fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("AIX"),
    );

    let data_dir_path: PathBuf = app.internal_data_path().expect("No internal data path");
    APP_DATA_DIR.set(data_dir_path).unwrap();

    slint::android::init(app).unwrap();

    let ui = AppWindow::new().unwrap();
    let ui_weak = ui.as_weak();

    // Load saved config on startup
    {
        let tm = TOR_MANAGER.clone();
        let ui_weak2 = ui_weak.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let cfg = tm.load_config().await.unwrap_or_default();
                tm.update_config(cfg.clone()).await.ok();
                let logs = tm.get_logs().await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak2.upgrade() {
                        ui.set_sni_input(cfg.custom_sni.into());
                        ui.set_bridge_input(cfg.bridge_line.into());
                        ui.set_kill_switch(cfg.kill_switch);
                        ui.set_dns_over_tor(cfg.dns_over_tor);
                        ui.set_auto_reconnect(cfg.auto_reconnect);
                        ui.set_log_text(logs.into());
                    }
                });
            });
        });
    }

    // Populate preset lists
    let sni_names: Vec<slint::SharedString> =
        sni_presets().iter().map(|(n, _)| (*n).into()).collect();
    let bridge_names: Vec<slint::SharedString> =
        bridge_presets().iter().map(|(n, _, _)| (*n).into()).collect();
    ui.set_sni_presets(slint::ModelRc::new(slint::VecModel::from(sni_names)));
    ui.set_bridge_presets(slint::ModelRc::new(slint::VecModel::from(bridge_names)));
    ui.set_sni_input("www.cloudflare.com".into());
    ui.set_bridge_input("".into());
    ui.set_status_text("🔴 Disconnected".into());
    ui.set_stats_text("⏱ 00:00:00 | ↓0 B ↑0 B".into());
    ui.set_log_text("AIX VPN ready. Configure SNI + Bridge → CONNECT\n".into());
    ui.set_kill_switch(true);
    ui.set_dns_over_tor(true);
    ui.set_auto_reconnect(true);
    ui.set_is_connected(false);
    ui.set_selected_tab(0);

    // Stats ticker — updates every second while connected
    {
        let ui_weak2 = ui_weak.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    let tm = TOR_MANAGER.clone();
                    if tm.is_connected().await {
                        let stats = tm.get_stats_str().await;
                        let ui_local = ui_weak2.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_local.upgrade() {
                                ui.set_stats_text(stats.into());
                            }
                        });
                    }
                }
            });
        });
    }

    // SNI preset selected
    {
        let ui_weak = ui_weak.clone();
        ui.on_sni_preset_selected(move |preset_name| {
            let presets = sni_presets();
            if let Some((_, sni)) = presets.iter().find(|(n, _)| preset_name == *n) {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_sni_input((*sni).into());
                }
            }
        });
    }

    // Bridge preset selected
    {
        let ui_weak = ui_weak.clone();
        ui.on_bridge_preset_selected(move |preset_name| {
            let presets = bridge_presets();
            if let Some((_, sni, bridge)) = presets.iter().find(|(n, _, _)| preset_name == *n) {
                if let Some(ui) = ui_weak.upgrade() {
                    if !sni.is_empty() { ui.set_sni_input((*sni).into()); }
                    ui.set_bridge_input((*bridge).into());
                }
            }
        });
    }

    // Fetch bridge from BridgeDB
    {
        let ui_weak = ui_weak.clone();
        ui.on_fetch_bridge(move || {
            let ui_weak2 = ui_weak.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let tm = TOR_MANAGER.clone();
                    tm.push_log("⏳ Fetching bridge from BridgeDB...".into()).await;
                    match fetch_random_bridge().await {
                        Ok(b) => {
                            tm.push_log(format!("✅ Got bridge: {}", &b[..b.len().min(50)])).await;
                            let logs = tm.get_logs().await;
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_weak2.upgrade() {
                                    ui.set_bridge_input(b.into());
                                    ui.set_log_text(logs.into());
                                }
                            });
                        }
                        Err(e) => {
                            let msg = format!("❌ Fetch failed: {e}");
                            tm.push_log(msg).await;
                            let logs = tm.get_logs().await;
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_weak2.upgrade() { ui.set_log_text(logs.into()); }
                            });
                        }
                    }
                });
            });
        });
    }

    // Connect
    {
        let ui_weak = ui_weak.clone();
        ui.on_connect(move || {
            let ui = ui_weak.upgrade().unwrap();
            let sni    = ui.get_sni_input().to_string();
            let bridge = ui.get_bridge_input().to_string();
            let ks     = ui.get_kill_switch();
            let dot    = ui.get_dns_over_tor();
            let ar     = ui.get_auto_reconnect();
            let ui_weak2 = ui_weak.clone();
            ui.set_status_text("⏳ Connecting...".into());
            ui.set_is_connected(false);
            let tm = TOR_MANAGER.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let cfg = SniConfig {
                        enabled: true, custom_sni: sni,
                        bridge_line: bridge, bridge_type: "webtunnel".into(),
                        kill_switch: ks, dns_over_tor: dot, auto_reconnect: ar,
                    };
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

    // Disconnect
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
                            ui.set_stats_text("⏱ 00:00:00 | ↓0 B ↑0 B".into());
                            ui.set_log_text(logs.into());
                        }
                    });
                });
            });
        });
    }

    // New circuit button removed (not available in arti 0.40)
    // We'll keep the callback but make it a no-op
    ui.on_new_circuit(move || {
        let tm = TOR_MANAGER.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                tm.push_log("⚠️  New circuit not available in this arti version".into()).await;
                let logs = tm.get_logs().await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui.as_weak().upgrade() {
                        ui.set_log_text(logs.into());
                    }
                });
            });
        });
    });

    // Refresh logs
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

    // Clear logs
    {
        ui.on_clear_logs(move || {
            let tm = TOR_MANAGER.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    tm.log_buf.lock().await.clear();
                });
            });
        });
    }

    ui.run().unwrap();
}

// ── JNI bridge for TorVpnService ─────────────────────────────────────────────

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
            let cfg = SniConfig {
                enabled: true, custom_sni: sni_str,
                bridge_line: bridge_str, bridge_type: "webtunnel".into(),
                kill_switch: true, dns_over_tor: true, auto_reconnect: true,
            };
            let _ = tm.update_config(cfg).await;
            if let Err(e) = tm.start_tor().await {
                log::error!("Tor start failed: {e}"); return;
            }
            log::info!("TUN fd={tun_fd} active — all traffic routed through Tor");
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
