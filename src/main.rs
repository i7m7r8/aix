use eframe::egui;
use std::io::Read;

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default(),
        ..Default::default()
    };
    eframe::run_native("Rust AI Debugger", options, Box::new(|_cc| Box::new(AIDebugApp::default())));
}

struct AIDebugApp {
    chat_input: String,
    chat_history: Vec<(String, String)>,
    zip_path: String,
    debug_log: String,
}

impl Default for AIDebugApp {
    fn default() -> Self {
        Self {
            chat_input: String::new(),
            chat_history: Vec::new(),
            zip_path: String::from("/sdcard/Download/source.zip"),
            debug_log: String::from("Ready to debug zipped source..."),
        }
    }
}

impl eframe::App for AIDebugApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🛠️ Rust FOSS AI Debugger");
            ui.separator();
            
            egui::ScrollArea::vertical().stick_to_bottom(true).max_height(300.0).show(ui, |ui| {
                for (user, msg) in &self.chat_history {
                    ui.label(format!("{}: {}", user, msg));
                }
            });

            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.chat_input);
                if ui.button("Send").clicked() {
                    let msg = self.chat_input.clone();
                    self.chat_history.push(("User".to_string(), msg));
                    self.chat_input.clear();
                    self.chat_history.push(("AI".to_string(), "Analyzing source context...".to_string()));
                }
            });

            ui.separator();
            ui.label("Path to Source Zip:");
            ui.text_edit_singleline(&mut self.zip_path);
            
            if ui.button("Extract & Debug").clicked() {
                self.debug_log = self.inspect_zip();
            }

            ui.label("Debug Output:");
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(egui::Label::new(&self.debug_log).wrap(true));
            });
        });
    }
}

impl AIDebugApp {
    fn inspect_zip(&self) -> String {
        let file = match std::fs::File::open(&self.zip_path) {
            Ok(f) => f,
            Err(e) => return format!("File Error: {}", e),
        };
        let mut archive = match zip::ZipArchive::new(file) {
            Ok(a) => a,
            Err(e) => return format!("Zip Error: {}", e),
        };
        let mut res = format!("Analyzing {} files...\n", archive.len());
        for i in 0..archive.len() {
            let file = archive.by_index(i).unwrap();
            res.push_str(&format!("- {}\n", file.name()));
        }
        res
    }
}
