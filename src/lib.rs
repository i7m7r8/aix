use eframe::egui;
use std::sync::{Arc, Mutex};
use std::path::{Path, PathBuf};
use std::process::Command;
use chrono::{Local};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
enum AppTab { Chat, Shell, FileExplorer, Hardware, Advanced }

#[derive(Serialize, Deserialize, Clone)]
struct ChatMsg {
    id: usize,
    sender: String,
    text: String,
    time: String,
}

struct HardwareSnapshot {
    battery: String,
    is_root: bool,
    kernel: String,
}

struct AixState {
    tab: AppTab,
    history: Vec<ChatMsg>,
    input_buffer: String,
    system_logs: Vec<String>,
    hw: HardwareSnapshot,
    dark_mode: bool,
    active_dir: PathBuf,
}

pub struct AixUltraApp {
    state: Arc<Mutex<AixState>>,
}

impl AixUltraApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        android_logger::init_once(android_logger::Config::default().with_tag("AIX_ENGINE"));
        let initial_state = AixState {
            tab: AppTab::Chat,
            history: vec![ChatMsg {
                id: 0,
                sender: "AIX".into(),
                text: "Ultra Kernel Online.".into(),
                time: Local::now().format("%H:%M").to_string(),
            }],
            input_buffer: String::new(),
            system_logs: vec!["[BOOT] System ready.".into()],
            hw: HardwareSnapshot { battery: "0%".into(), is_root: false, kernel: "Linux".into() },
            dark_mode: true,
            active_dir: PathBuf::from("/sdcard"),
        };
        Self { state: Arc::new(Mutex::new(initial_state)) }
    }

    fn run_sh(&self, cmd: &str) -> String {
        let output = Command::new("sh").arg("-c").arg(cmd).output();
        match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(_) => "ERR".into(),
        }
    }
}

impl eframe::App for AixUltraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut s = self.state.lock().unwrap();
        ctx.set_visuals(if s.dark_mode { egui::Visuals::dark() } else { egui::Visuals::light() });

        egui::TopBottomPanel::top("nav").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("⚡ AIX ULTRA");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("🌙").clicked() { s.dark_mode = !s.dark_mode; }
                });
            });
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.tab, AppTab::Chat, "💬 CHAT");
                ui.selectable_value(&mut s.tab, AppTab::Shell, "🐚 SHELL");
                ui.selectable_value(&mut s.tab, AppTab::Hardware, "📊 SYS");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match s.tab {
                AppTab::Chat => {
                    egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                        for m in &s.history {
                            ui.group(|ui| {
                                ui.label(format!("{}: {}", m.sender, m.text));
                            });
                        }
                    });
                }
                AppTab::Shell => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for log in &s.system_logs { ui.monospace(log); }
                    });
                }
                AppTab::Hardware => {
                    ui.label(format!("Battery: {}", s.hw.battery));
                    if ui.button("Refresh").clicked() {
                        s.hw.battery = self.run_sh("termux-battery-status | grep percentage");
                    }
                }
                _ => { ui.label("Coming soon..."); }
            }
        });

        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let te = ui.text_edit_singleline(&mut s.input_buffer);
                if ui.button("SEND").clicked() || (te.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))) {
                    let input = s.input_buffer.clone();
                    if s.tab == AppTab::Chat {
                        s.history.push(ChatMsg { id: 0, sender: "USER".into(), text: input, time: "".into() });
                    } else if s.tab == AppTab::Shell {
                        let res = self.run_sh(&input);
                        s.system_logs.push(format!("$ {}", input));
                        s.system_logs.push(res);
                    }
                    s.input_buffer.clear();
                }
            });
        });
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    let options = eframe::NativeOptions::default();
    eframe::run_native("AIX Ultra", options, Box::new(|cc| Ok(Box::new(AixUltraApp::new(cc))))).unwrap();
}
