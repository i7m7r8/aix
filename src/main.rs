#![allow(non_snake_case)]

use cranpose::prelude::*;
use tokio::sync::mpsc;
use std::sync::Arc;

use aix::{SniConfig, TOR_MANAGER};

#[derive(Clone, Debug)]
enum UIMessage {
    Status(String),
    Log(String),
    ConnectionStarted,
    ConnectionStopped,
}

fn main() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("AIX"),
    );

    AppLauncher::new()
        .with_title("AIX VPN")
        .run(|| app())
}

#[composable]
fn app() -> Element {
    let status = use_state(|| "🔴 Disconnected".to_string());
    let sni_input = use_state(|| "www.cloudflare.com".to_string());
    let bridge_input = use_state(|| "webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=...".to_string());
    let log_text = use_state(|| "Edit Custom SNI or use presets below → then tap CONNECT".to_string());
    let is_connected = use_state(|| false);

    // Channel for async UI updates
    let (tx, mut rx) = use_state(|| {
        let (tx, rx) = mpsc::unbounded_channel();
        (tx, rx)
    });

    // Spawn a task to listen for UI messages
    let status_clone = status.clone();
    let log_clone = log_text.clone();
    let connected_clone = is_connected.clone();
    let rx = rx.clone();
    use_effect(move || {
        let mut rx = rx;
        let status = status_clone;
        let log = log_clone;
        let connected = connected_clone;
        spawn(async move {
            while let Some(msg) = rx.recv().await {
                match msg {
                    UIMessage::Status(s) => status.set(s),
                    UIMessage::Log(s) => log.set(s),
                    UIMessage::ConnectionStarted => connected.set(true),
                    UIMessage::ConnectionStopped => connected.set(false),
                }
            }
        });
        || {}
    });

    let connect = move |_| {
        let sni = sni_input.get().clone();
        let bridge = bridge_input.get().clone();
        let tx = tx.clone();
        let status = status.clone();
        let log = log_text.clone();
        let connected = is_connected.clone();
        log.set("Updating SNI and starting Tor...".to_string());
        status.set("🟡 Connecting...".to_string());

        spawn(async move {
            let cfg = SniConfig {
                enabled: true,
                custom_sni: sni,
                bridge_line: bridge,
                last_updated: None,
            };
            if let Err(e) = TOR_MANAGER.update_sni(cfg).await {
                let _ = tx.send(UIMessage::Log(format!("SNI error: {}", e)));
                return;
            }
            match TOR_MANAGER.start_tor().await {
                Ok(msg) => {
                    let _ = tx.send(UIMessage::Status(msg));
                    let _ = tx.send(UIMessage::ConnectionStarted);
                    let _ = tx.send(UIMessage::Log("✅ Tor + Custom SNI started!".to_string()));
                }
                Err(e) => {
                    let _ = tx.send(UIMessage::Status(format!("❌ Failed: {}", e)));
                    let _ = tx.send(UIMessage::Log(format!("Error: {}", e)));
                }
            }
        });
    };

    let disconnect = move |_| {
        let tx = tx.clone();
        spawn(async move {
            TOR_MANAGER.stop_tor().await;
            let _ = tx.send(UIMessage::Status("🔴 Disconnected".to_string()));
            let _ = tx.send(UIMessage::ConnectionStopped);
            let _ = tx.send(UIMessage::Log("Tor stopped.".to_string()));
        });
    };

    Column(Modifier::fill_max_size().padding(20.0), || {
        // Header
        Row(Modifier::fill_width().align(Alignment::Center), || {
            Text("AIX VPN").font_size(32);
            Spacer(Modifier::grow(1.0));
            Text(status.get().clone()).font_size(16);
        });

        Spacer(Modifier::height(20.0));

        // SNI Input
        Text("🎯 Custom SNI").font_size(18);
        TextField {
            value: sni_input.get().clone(),
            placeholder: "www.example.com".into(),
            on_input: move |v| sni_input.set(v),
        };
        Text("Used for SNI imitation in pluggable transports")
            .font_size(12)
            .color(0x808080);

        Spacer(Modifier::height(10.0));

        // Quick Presets
        Text("⚡ Quick Presets").font_size(16);
        Row(Modifier::wrap(), || {
            Button("Cloudflare SNI", |_| {
                sni_input.set("www.cloudflare.com".to_string());
                bridge_input.set("webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=...".to_string());
                log_text.set("✅ Loaded Cloudflare SNI preset".to_string());
            });
            Button("VK.ru SNI", |_| {
                sni_input.set("vk.ru".to_string());
                bridge_input.set("webtunnel [2a0a:0:0:0::1]:443 sni-imitation=vk.ru fingerprint=...".to_string());
                log_text.set("✅ Loaded VK.ru SNI".to_string());
            });
            Button("Microsoft SNI", |_| {
                sni_input.set("www.microsoft.com".to_string());
                bridge_input.set("webtunnel 185.220.101.2:443 sni-imitation=www.microsoft.com fingerprint=...".to_string());
                log_text.set("✅ Loaded Microsoft SNI".to_string());
            });
            Button("Yandex SNI", |_| {
                sni_input.set("ya.ru".to_string());
                bridge_input.set("webtunnel 185.220.101.3:443 sni-imitation=ya.ru fingerprint=...".to_string());
                log_text.set("✅ Loaded Yandex SNI".to_string());
            });
        });
        Text("Tap preset → edit SNI freely → then CONNECT")
            .font_size(12)
            .color(0x808080);

        Spacer(Modifier::height(10.0));

        // Bridge Line
        Text("🌉 Bridge Line").font_size(16);
        TextField {
            value: bridge_input.get().clone(),
            placeholder: "bridge line...".into(),
            on_input: move |v| bridge_input.set(v),
        };

        Spacer(Modifier::height(10.0));

        // Log area
        Scrollable(Modifier::height(150.0), || {
            Text(log_text.get().clone())
                .font_size(12)
                .color(0x4CAF50);
        });

        Spacer(Modifier::height(20.0));

        // Buttons
        Row(Modifier::align(Alignment::Center), || {
            Button("CONNECT", connect).primary();
            Button("DISCONNECT", disconnect).destructive();
        });

        Spacer(Modifier::height(20.0));

        // Bottom navigation
        Row(Modifier::align(Alignment::Center).spacing(20.0), || {
            Text("🏠 Home");
            Text("🌉 Bridges");
            Text("⚙️ Settings");
        });
    })
}
