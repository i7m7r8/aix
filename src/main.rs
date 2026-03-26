#![allow(non_snake_case)]

use iced::{
    widget::{button, column, container, row, scrollable, text, text_input, Column, Row, Space},
    Alignment, Element, Length, Task,
};
use tokio::sync::mpsc;
use std::sync::Arc;

use aix::{SniConfig, TOR_MANAGER};

// Messages sent from the async task to the UI thread
#[derive(Debug, Clone)]
enum UIMessage {
    Status(String),
    Log(String),
    ConnectionStarted,
    ConnectionStopped,
}

// Internal app state
struct AixVpn {
    status: String,
    sni_input: String,
    bridge_input: String,
    log: String,
    is_connected: bool,
    ui_tx: Option<mpsc::UnboundedSender<UIMessage>>,
    // For receiving messages from the async task
    ui_rx: Option<mpsc::UnboundedReceiver<UIMessage>>,
}

#[derive(Debug, Clone)]
enum Message {
    Connect,
    Disconnect,
    SniChanged(String),
    BridgeChanged(String),
    Preset(String, String),
    UIEvent(UIMessage),
}

impl iced::Application for AixVpn {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = iced::Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Task<Message>) {
        let (ui_tx, ui_rx) = mpsc::unbounded_channel();
        let app = Self {
            status: "🔴 Disconnected".to_string(),
            sni_input: "www.cloudflare.com".to_string(),
            bridge_input: "webtunnel 185.220.101.1:443 sni-imitation=www.cloudflare.com fingerprint=...".to_string(),
            log: "Edit Custom SNI or use presets below → then tap CONNECT".to_string(),
            is_connected: false,
            ui_tx: Some(ui_tx),
            ui_rx: Some(ui_rx),
        };
        (app, Task::none())
    }

    fn title(&self) -> String {
        String::from("AIX VPN")
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Connect => {
                let sni = self.sni_input.clone();
                let bridge = self.bridge_input.clone();
                let tx = self.ui_tx.clone().unwrap();
                self.log = "Updating SNI and starting Tor...".to_string();
                self.status = "🟡 Connecting...".to_string();

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
                            let _ = tx.send(UIMessage::ConnectionStarted);
                            let _ = tx.send(UIMessage::Log("✅ Tor + Custom SNI started!".to_string()));
                        }
                        Err(e) => {
                            let _ = tx.send(UIMessage::Status(format!("❌ Failed: {}", e)));
                            let _ = tx.send(UIMessage::Log(format!("Error: {}", e)));
                        }
                    }
                });
                Task::none()
            }
            Message::Disconnect => {
                let tx = self.ui_tx.clone().unwrap();
                tokio::spawn(async move {
                    TOR_MANAGER.stop_tor().await;
                    let _ = tx.send(UIMessage::Status("🔴 Disconnected".to_string()));
                    let _ = tx.send(UIMessage::ConnectionStopped);
                    let _ = tx.send(UIMessage::Log("Tor stopped.".to_string()));
                });
                Task::none()
            }
            Message::SniChanged(s) => {
                self.sni_input = s;
                Task::none()
            }
            Message::BridgeChanged(b) => {
                self.bridge_input = b;
                Task::none()
            }
            Message::Preset(sni, bridge) => {
                self.sni_input = sni;
                self.bridge_input = bridge;
                self.log = format!("✅ Loaded preset: {} SNI", sni);
                Task::none()
            }
            Message::UIEvent(ui_msg) => {
                match ui_msg {
                    UIMessage::Status(s) => self.status = s,
                    UIMessage::Log(s) => self.log = s,
                    UIMessage::ConnectionStarted => self.is_connected = true,
                    UIMessage::ConnectionStopped => self.is_connected = false,
                }
                Task::none()
            }
        }
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        // Poll the channel for UI events
        let rx = self.ui_rx.as_ref().unwrap().clone();
        iced::subscription::unfold(rx, |mut rx| async {
            rx.recv().await.map(|msg| (Message::UIEvent(msg), rx))
        })
    }

    fn view(&self) -> Element<Message> {
        let header = row![
            text("AIX VPN").size(32),
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

pub fn main() -> iced::Result {
    // Initialize Android logging
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("AIX"),
    );

    // Run Iced application
    iced::application("AIX VPN", AixVpn::update, AixVpn::view)
        .subscription(AixVpn::subscription)
        .theme(|_| iced::Theme::Dark)
        .run()
}
