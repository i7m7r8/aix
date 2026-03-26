#![allow(non_snake_case)]

use iced::{
    widget::{button, column, container, row, scrollable, text, text_input, Column, Row, Space},
    Alignment, Element, Length, Sandbox, Settings,
};
use tokio::sync::Mutex;
use std::sync::Arc;

use aix::{SniConfig, TOR_MANAGER};

pub fn main() -> iced::Result {
    // Initialize Android logging
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("AIX"),
    );

    // Run Iced application on Android
    iced::Application::run(Settings {
        antialiasing: true,
        ..Settings::default()
    })
}

struct AixVpn {
    status: String,
    sni_input: String,
    bridge_input: String,
    log: String,
    is_connected: bool,
}

#[derive(Debug, Clone)]
enum Message {
    Connect,
    Disconnect,
    SniChanged(String),
    BridgeChanged(String),
    Preset(String, String),
    UpdateStatus(String),
    UpdateLog(String),
    ConnectionStarted,
    ConnectionStopped,
}

impl Sandbox for AixVpn {
    type Message = Message;

    fn new() -> Self {
        Self {
            status: "🔴 Disconnected".to_string(),
            sni_input: "www.cloudflare.com".to_string(),
            bridge_input: "webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=...".to_string(),
            log: "Edit Custom SNI or use presets below → then tap CONNECT".to_string(),
            is_connected: false,
        }
    }

    fn title(&self) -> String {
        String::from("AIX VPN")
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Connect => {
                let sni = self.sni_input.clone();
                let bridge = self.bridge_input.clone();
                self.log = "Updating SNI and starting Tor...".to_string();
                self.status = "🟡 Connecting...".to_string();

                let tm = TOR_MANAGER.clone();
                tokio::spawn(async move {
                    let cfg = SniConfig {
                        enabled: true,
                        custom_sni: sni,
                        bridge_line: bridge,
                        last_updated: None,
                    };
                    if let Err(e) = tm.update_sni(cfg).await {
                        // Handle error (send message back to UI)
                        // For simplicity, we'll use a channel later
                        eprintln!("SNI error: {}", e);
                        return;
                    }
                    match tm.start_tor().await {
                        Ok(msg) => {
                            // Update status (need to send message)
                        }
                        Err(e) => {
                            // Handle error
                        }
                    }
                });
                // TODO: send messages back to UI (use a channel or async runtime)
            }
            Message::Disconnect => {
                let tm = TOR_MANAGER.clone();
                tokio::spawn(async move {
                    tm.stop_tor().await;
                });
            }
            Message::SniChanged(s) => self.sni_input = s,
            Message::BridgeChanged(b) => self.bridge_input = b,
            Message::Preset(sni, bridge) => {
                self.sni_input = sni;
                self.bridge_input = bridge;
                self.log = format!("✅ Loaded preset: {} SNI", sni);
            }
            Message::UpdateStatus(s) => self.status = s,
            Message::UpdateLog(s) => self.log = s,
            Message::ConnectionStarted => self.is_connected = true,
            Message::ConnectionStopped => self.is_connected = false,
        }
    }

    fn view(&self) -> Element<Message> {
        let header = row![
            text("AIX VPN").size(32).font(iced::Font::MONOSPACE),
            Space::with_width(Length::Fill),
            text(&self.status).size(16),
        ]
        .align_items(Alignment::Center)
        .padding(20);

        let sni_section = column![
            text("🎯 Custom SNI").size(18),
            text_input("www.example.com", &self.sni_input).on_input(Message::SniChanged),
            text("Used for SNI imitation in pluggable transports").size(12).style(iced::theme::Text::Color(iced::Color::from_rgb(0.5, 0.5, 0.5))),
        ]
        .spacing(10);

        let presets_section = column![
            text("⚡ Quick Presets").size(16),
            row![
                button("Cloudflare SNI").on_press(Message::Preset(
                    "www.cloudflare.com".to_string(),
                    "webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=...".to_string()
                )),
                button("VK.ru SNI").on_press(Message::Preset(
                    "vk.ru".to_string(),
                    "webtunnel [2a0a:0:0:0::1]:443 sni-imitation=vk.ru fingerprint=...".to_string()
                )),
                button("Microsoft SNI").on_press(Message::Preset(
                    "www.microsoft.com".to_string(),
                    "webtunnel 185.220.101.2:443 sni-imitation=www.microsoft.com fingerprint=...".to_string()
                )),
                button("Yandex SNI").on_press(Message::Preset(
                    "ya.ru".to_string(),
                    "webtunnel 185.220.101.3:443 sni-imitation=ya.ru fingerprint=...".to_string()
                )),
            ]
            .spacing(5)
            .wrap(),
            text("Tap preset → edit SNI freely → then CONNECT").size(12).style(iced::theme::Text::Color(iced::Color::from_rgb(0.5, 0.5, 0.5))),
        ]
        .spacing(10);

        let bridge_section = column![
            text("🌉 Bridge Line").size(16),
            text_input("bridge line...", &self.bridge_input)
                .on_input(Message::BridgeChanged)
                .padding(10)
                .height(Length::Units(80)),
        ]
        .spacing(10);

        let log_section = scrollable(
            text(&self.log)
                .size(12)
                .style(iced::theme::Text::Color(iced::Color::from_rgb(0.3, 0.8, 0.3)))
        )
        .height(Length::Units(150))
        .padding(10);

        let buttons = row![
            button("CONNECT")
                .on_press(Message::Connect)
                .style(iced::theme::Button::Primary)
                .padding(10),
            button("DISCONNECT")
                .on_press(Message::Disconnect)
                .style(iced::theme::Button::Destructive)
                .padding(10),
        ]
        .spacing(20)
        .align_items(Alignment::Center);

        let bottom_nav = row![
            text("🏠 Home"),
            text("🌉 Bridges"),
            text("⚙️ Settings"),
        ]
        .spacing(20)
        .align_items(Alignment::Center);

        let content = column![
            header,
            sni_section,
            presets_section,
            bridge_section,
            log_section,
            buttons,
            bottom_nav,
        ]
        .spacing(20)
        .padding(20);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x()
            .into()
    }
}
