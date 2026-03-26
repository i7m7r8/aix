use eframe::egui;
use tokio::sync::mpsc;
use std::sync::Arc;

use aix::{SniConfig, TOR_MANAGER};

// Messages from async tasks to the UI thread
#[derive(Clone, Debug)]
enum UIMessage {
    Status(String),
    Log(String),
    Connected,
    Disconnected,
}

struct AixApp {
    status: String,
    sni_input: String,
    bridge_input: String,
    log_text: String,
    is_connected: bool,
    tx: mpsc::UnboundedSender<UIMessage>,
    rx: mpsc::UnboundedReceiver<UIMessage>,
}

impl AixApp {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            status: "🔴 Disconnected".to_string(),
            sni_input: "www.cloudflare.com".to_string(),
            bridge_input: "webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=...".to_string(),
            log_text: "Edit Custom SNI → tap CONNECT".to_string(),
            is_connected: false,
            tx,
            rx,
        }
    }

    fn poll_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                UIMessage::Status(s) => self.status = s,
                UIMessage::Log(s) => self.log_text = s,
                UIMessage::Connected => self.is_connected = true,
                UIMessage::Disconnected => self.is_connected = false,
            }
        }
    }

    fn connect(&mut self) {
        let sni = self.sni_input.clone();
        let bridge = self.bridge_input.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
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
                    let _ = tx.send(UIMessage::Connected);
                    let _ = tx.send(UIMessage::Log("✅ Tor started".to_string()));
                }
                Err(e) => {
                    let _ = tx.send(UIMessage::Status(format!("❌ Failed: {}", e)));
                    let _ = tx.send(UIMessage::Log(format!("Error: {}", e)));
                }
            }
        });
    }

    fn disconnect(&mut self) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            TOR_MANAGER.stop_tor().await;
            let _ = tx.send(UIMessage::Status("🔴 Disconnected".to_string()));
            let _ = tx.send(UIMessage::Disconnected);
            let _ = tx.send(UIMessage::Log("Tor stopped".to_string()));
        });
    }
}

impl eframe::App for AixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_messages();

        // Request repaint continuously to keep UI responsive
        ctx.request_repaint();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("AIX VPN");
                ui.horizontal_wrapped(|ui| {
                    ui.label("Status:");
                    ui.colored_label(
                        if self.is_connected {
                            egui::Color32::GREEN
                        } else {
                            egui::Color32::RED
                        },
                        &self.status,
                    );
                });
            });

            ui.separator();

            // SNI Section
            ui.add_space(10.0);
            ui.label("🎯 Custom SNI");
            ui.text_edit_singleline(&mut self.sni_input);
            ui.label("Used for SNI imitation in pluggable transports")
                .small();

            ui.add_space(10.0);

            // Quick Presets
            ui.label("⚡ Quick Presets");
            ui.horizontal_wrapped(|ui| {
                if ui.button("Cloudflare SNI").clicked() {
                    self.sni_input = "www.cloudflare.com".to_string();
                    self.bridge_input = "webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=...".to_string();
                    self.log_text = "✅ Loaded Cloudflare SNI preset".to_string();
                }
                if ui.button("VK.ru SNI").clicked() {
                    self.sni_input = "vk.ru".to_string();
                    self.bridge_input = "webtunnel [2a0a:0:0:0::1]:443 sni-imitation=vk.ru fingerprint=...".to_string();
                    self.log_text = "✅ Loaded VK.ru SNI".to_string();
                }
                if ui.button("Microsoft SNI").clicked() {
                    self.sni_input = "www.microsoft.com".to_string();
                    self.bridge_input = "webtunnel 185.220.101.2:443 sni-imitation=www.microsoft.com fingerprint=...".to_string();
                    self.log_text = "✅ Loaded Microsoft SNI".to_string();
                }
                if ui.button("Yandex SNI").clicked() {
                    self.sni_input = "ya.ru".to_string();
                    self.bridge_input = "webtunnel 185.220.101.3:443 sni-imitation=ya.ru fingerprint=...".to_string();
                    self.log_text = "✅ Loaded Yandex SNI".to_string();
                }
            });
            ui.label("Tap preset → edit SNI freely → then CONNECT").small();

            ui.add_space(10.0);

            // Bridge Line
            ui.label("🌉 Bridge Line");
            ui.text_edit_multiline(&mut self.bridge_input)
                .desired_width(f32::INFINITY)
                .desired_rows(3);

            ui.add_space(10.0);

            // Log area
            ui.label("Log");
            egui::ScrollArea::vertical()
                .max_height(150.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.log_text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(5)
                            .interactive(false),
                    );
                });

            ui.add_space(10.0);

            // Connect / Disconnect buttons
            ui.horizontal(|ui| {
                if ui.button("CONNECT").clicked() {
                    self.connect();
                }
                if ui.button("DISCONNECT").clicked() {
                    self.disconnect();
                }
            });

            ui.add_space(10.0);

            // Bottom navigation
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label("🏠 Home");
                ui.add_space(20.0);
                ui.label("🌉 Bridges");
                ui.add_space(20.0);
                ui.label("⚙️ Settings");
            });
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    // Initialize Android logging
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("AIX"),
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([450.0, 750.0]),
        ..Default::default()
    };

    eframe::run_native(
        "AIX VPN",
        options,
        Box::new(|_cc| Box::new(AixApp::new())),
    )
}
