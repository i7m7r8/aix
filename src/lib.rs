//! AIX Ultra – A local AI chat and source code debugger for Android.
//! Runs entirely on‑device using Candle for LLM inference.

#![cfg_attr(target_os = "android", allow(unused_imports))]

use anyhow::{anyhow, Result};
use chrono::Local;
use directories::ProjectDirs;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;
use zip::read::ZipArchive;

// Candle imports
use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::quantized_llama as model;
use candle_transformers::models::quantized_llama::{Config, Model};

// Syntect imports for syntax highlighting
use syntect::easy::HighlightLines;
use syntect::highlighting::{ThemeSet, Style};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

// -----------------------------------------------------------------------------
// App configuration and state
// -----------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
enum AppTab {
    Chat,
    Shell,
    Hardware,
    FileBrowser,
    Editor,
    Settings,
}

#[derive(Serialize, Deserialize, Clone)]
struct ChatMessage {
    sender: String,
    text: String,
    time: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct Settings {
    dark_mode: bool,
    model_path: PathBuf,
    auto_save: bool,
    chat_history_file: PathBuf,
}

impl Default for Settings {
    fn default() -> Self {
        let proj = ProjectDirs::from("com", "i7m7r8", "AIX").unwrap();
        let data_dir = proj.data_dir();
        Self {
            dark_mode: true,
            model_path: data_dir.join("models").join("tinyllama-1.1b-chat-v1.0"),
            auto_save: true,
            chat_history_file: data_dir.join("chat_history.json"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct FileEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    size: u64,
    modified: SystemTime,
}

struct EditorState {
    current_file: Option<PathBuf>,
    content: String,
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    highlighter: Option<HighlightLines<'static>>, // use 'static lifetime
}

impl EditorState {
    fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        Self {
            current_file: None,
            content: String::new(),
            syntax_set,
            theme_set,
            highlighter: None,
        }
    }

    fn load_file(&mut self, path: &Path) -> Result<()> {
        let content = fs::read_to_string(path)?;
        self.content = content;
        self.current_file = Some(path.to_path_buf());
        self.update_highlighter();
        Ok(())
    }

    fn save_file(&mut self) -> Result<()> {
        if let Some(path) = &self.current_file {
            fs::write(path, &self.content)?;
            Ok(())
        } else {
            Err(anyhow!("No file loaded"))
        }
    }

    fn update_highlighter(&mut self) {
        if let Some(path) = &self.current_file {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let syntax = self
                .syntax_set
                .find_syntax_by_extension(ext)
                .or_else(|| self.syntax_set.find_syntax_by_extension("txt"))
                .unwrap();
            // Leak the theme to get a 'static reference (ok for a short-lived demo)
            let theme = &self.theme_set.themes["base16-ocean.dark"];
            let theme_static: &'static syntect::highlighting::Theme = Box::leak(Box::new(theme.clone()));
            self.highlighter = Some(HighlightLines::new(syntax, theme_static));
        } else {
            self.highlighter = None;
        }
    }
}

struct ZipDebugger {
    extracted_dir: Option<PathBuf>,
    analysis: Vec<String>,
    warnings: Vec<String>,
    errors: Vec<String>,
}

impl ZipDebugger {
    fn new() -> Self {
        Self {
            extracted_dir: None,
            analysis: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn extract_zip(&mut self, zip_path: &Path) -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let dest = temp_dir.path();
        let file = File::open(zip_path)?;
        let mut archive = ZipArchive::new(file)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let outpath = dest.join(file.name());
            if file.is_dir() {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        fs::create_dir_all(p)?;
                    }
                }
                let mut outfile = File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }
        self.extracted_dir = Some(dest.to_path_buf());
        self.analyze_code();
        Ok(())
    }

    fn analyze_code(&mut self) {
        self.analysis.clear();
        self.warnings.clear();
        self.errors.clear();

        if let Some(dir) = &self.extracted_dir {
            for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if matches!(ext, "rs" | "c" | "cpp" | "h" | "py" | "java" | "js") {
                            if let Ok(content) = fs::read_to_string(path) {
                                self.analysis.push(format!("File: {}", path.display()));
                                // Simple static analysis: look for common issues
                                if content.contains("unsafe") {
                                    self.warnings.push(format!("{}: contains unsafe code", path.display()));
                                }
                                if content.contains("TODO") {
                                    self.warnings.push(format!("{}: contains TODO", path.display()));
                                }
                                if content.contains("panic!") {
                                    self.errors.push(format!("{}: contains panic! macro", path.display()));
                                }
                                // Check for missing error handling
                                if content.contains("unwrap()") && !content.contains("expect") {
                                    self.warnings.push(format!("{}: uses unwrap() without expect", path.display()));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn cleanup(&mut self) {
        if let Some(dir) = &self.extracted_dir {
            let _ = fs::remove_dir_all(dir);
            self.extracted_dir = None;
        }
        self.analysis.clear();
        self.warnings.clear();
        self.errors.clear();
    }
}

struct AiModel {
    device: Device,
    model: Option<Model>,
    tokenizer: Option<tokenizers::Tokenizer>,
    context_size: usize,
}

impl AiModel {
    fn new(settings: &Settings) -> Result<Self> {
        let device = Device::Cpu;
        let model_path = &settings.model_path;
        if !model_path.exists() {
            return Err(anyhow!("Model not found at {:?}", model_path));
        }

        // Load tokenizer
        let tokenizer_path = model_path.join("tokenizer.json");
        let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path).map_err(|e| anyhow!(e))?;

        // Load model weights
        let weights_path = model_path.join("model.safetensors");
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], candle_core::DType::F32, &device)? };

        let config_path = model_path.join("config.json");
        let config = Config::from_reader(File::open(config_path)?)?;
        let model = Model::new(&config, vb)?;

        Ok(Self {
            device,
            model: Some(model),
            tokenizer: Some(tokenizer),
            context_size: config.context_length,
        })
    }

    fn generate(&mut self, prompt: &str) -> Result<String> {
        let model = self.model.as_mut().ok_or_else(|| anyhow!("Model not loaded"))?;
        let tokenizer = self.tokenizer.as_ref().ok_or_else(|| anyhow!("Tokenizer not loaded"))?;

        let mut tokens = tokenizer.encode(prompt, true).map_err(|e| anyhow!(e))?.get_ids().to_vec();
        // Simple generation loop (very basic, real implementation would be more complex)
        let eos_token = tokenizer.token_to_id("</s>").unwrap_or(2);
        let max_tokens = 200;

        for _ in 0..max_tokens {
            // Prepare input tensor: shape [1, seq_len]
            let input_ids = Tensor::new(Tensor::new(&[&tokens],[tokens.as_slice()], &self.device)?;
            let logits = model.forward(&input_ids, 0)?;
            let next_token = logits.squeeze(0)?.argmax(0)?.to_scalar::<u32>()?;
            tokens.push(next_token);
            if next_token == eos_token {
                break;
            }
        }

        let output = tokenizer.decode(&tokens, true).map_err(|e| anyhow!(e))?;
        Ok(output)
    }
}

struct AixState {
    tab: AppTab,
    history: Vec<ChatMessage>,
    input: String,
    logs: Vec<String>,
    settings: Settings,
    file_browser_root: PathBuf,
    file_browser_current_dir: PathBuf,
    file_entries: Vec<FileEntry>,
    zip_debugger: ZipDebugger,
    editor: EditorState,
    model: Option<AiModel>,
    model_loading: bool,
    model_error: Option<String>,
}

impl AixState {
    fn new() -> Self {
        let settings = Settings::default();
        let proj = ProjectDirs::from("com", "i7m7r8", "AIX").unwrap();
        let data_dir = proj.data_dir();
        fs::create_dir_all(&data_dir).ok();

        Self {
            tab: AppTab::Chat,
            history: vec![ChatMessage {
                sender: "SYSTEM".into(),
                text: "AIX Ultra Kernel v1.0 Online".into(),
                time: Local::now().format("%H:%M").to_string(),
            }],
            input: String::new(),
            logs: vec!["[BOOT] Services started...".into()],
            settings,
            file_browser_root: PathBuf::from("/sdcard"),
            file_browser_current_dir: PathBuf::from("/sdcard"),
            file_entries: Vec::new(),
            zip_debugger: ZipDebugger::new(),
            editor: EditorState::new(),
            model: None,
            model_loading: false,
            model_error: None,
        }
    }

    fn refresh_file_browser(&mut self) {
        self.file_entries.clear();
        if let Ok(entries) = fs::read_dir(&self.file_browser_current_dir) {
            for entry in entries.flatten() {
                let metadata = entry.metadata().ok();
                let name = entry.file_name().to_string_lossy().to_string();
                let path = entry.path();
                let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                let modified = metadata
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                self.file_entries.push(FileEntry { name, path, is_dir, size, modified });
            }
            self.file_entries.sort_by(|a, b| a.name.cmp(&b.name));
        }
    }

    fn load_model(&mut self) {
        if self.model.is_some() || self.model_loading {
            return;
        }
        self.model_loading = true;
        let settings = self.settings.clone();
        let model_result = AiModel::new(&settings);
        match model_result {
            Ok(model) => {
                self.model = Some(model);
                self.logs.push("[AI] Model loaded successfully".into());
            }
            Err(e) => {
                self.model_error = Some(e.to_string());
                self.logs.push(format!("[AI] Model loading failed: {}", e));
            }
        }
        self.model_loading = false;
    }

    fn send_message(&mut self, text: &str) {
        let msg = ChatMessage {
            sender: "USER".into(),
            text: text.to_string(),
            time: Local::now().format("%H:%M").to_string(),
        };
        self.history.push(msg);
        self.input.clear();

        // Generate AI response if model is loaded
        if let Some(model) = &mut self.model {
            let prompt = format!("<|user|>\n{}\n<|assistant|>\n", text);
            match model.generate(&prompt) {
                Ok(response) => {
                    self.history.push(ChatMessage {
                        sender: "AI".into(),
                        text: response,
                        time: Local::now().format("%H:%M").to_string(),
                    });
                }
                Err(e) => {
                    self.history.push(ChatMessage {
                        sender: "ERROR".into(),
                        text: format!("Failed to generate: {}", e),
                        time: Local::now().format("%H:%M").to_string(),
                    });
                }
            }
        } else {
            self.history.push(ChatMessage {
                sender: "AI".into(),
                text: "Model not loaded. Please load a model in Settings.".into(),
                time: Local::now().format("%H:%M").to_string(),
            });
        }
    }

    fn shell_command(&self, cmd: &str) -> String {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(e) => format!("Error: {}", e),
        }
    }

    fn hardware_info(&self) -> String {
        let mut info = String::new();
        info.push_str("=== Hardware Info ===\n");
        info.push_str(&format!("Battery: {}\n", self.shell_command("termux-battery-status | grep percentage")));
        info.push_str(&format!("CPU: {}\n", self.shell_command("cat /proc/cpuinfo | grep 'processor' | wc -l")));
        info.push_str(&format!("Memory: {}\n", self.shell_command("free -h | grep Mem")));
        info.push_str(&format!("Storage: {}\n", self.shell_command("df -h /data")));
        info
    }
}

struct AixApp {
    state: Arc<Mutex<AixState>>,
}

impl AixApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        android_logger::init_once(android_logger::Config::default().with_tag("AIX"));
        let state = AixState::new();
        Self { state: Arc::new(Mutex::new(state)) }
    }

    fn render_chat(&self, ui: &mut egui::Ui, state: &mut AixState) {
        egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
            for msg in &state.history {
                ui.group(|ui| {
                    ui.label(format!("[{}] {}", msg.time, msg.sender));
                    ui.label(&msg.text);
                });
            }
        });
    }

    fn render_shell(&self, ui: &mut egui::Ui, state: &mut AixState) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for log in &state.logs {
                ui.monospace(log);
            }
        });
    }

    fn render_hardware(&self, ui: &mut egui::Ui, state: &mut AixState) {
        ui.label(state.hardware_info());
        if ui.button("Refresh").clicked() {
            // Refresh is done on next render
        }
    }

    fn render_file_browser(&self, ui: &mut egui::Ui, state: &mut AixState) {
        ui.horizontal(|ui| {
            ui.label("Current: ");
            ui.monospace(state.file_browser_current_dir.display().to_string());
            if ui.button("..").clicked() {
                if let Some(parent) = state.file_browser_current_dir.parent() {
                    state.file_browser_current_dir = parent.to_path_buf();
                    state.refresh_file_browser();
                }
            }
            if ui.button("Refresh").clicked() {
                state.refresh_file_browser();
            }
        });
        ui.separator();

        // Clone entries to avoid borrowing conflict
        let entries = state.file_entries.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for entry in &entries {
                ui.horizontal(|ui| {
                    let icon = if entry.is_dir { "📁" } else { "📄" };
                    if ui.button(format!("{} {}", icon, entry.name)).clicked() {
                        if entry.is_dir {
                            state.file_browser_current_dir = entry.path.clone();
                            state.refresh_file_browser();
                        } else {
                            // Open file in editor if it's a text file
                            if let Some(ext) = entry.path.extension().and_then(|e| e.to_str()) {
                                if matches!(ext, "rs" | "c" | "cpp" | "h" | "py" | "java" | "js" | "txt" | "md") {
                                    if let Err(e) = state.editor.load_file(&entry.path) {
                                        state.logs.push(format!("Failed to load file: {}", e));
                                    } else {
                                        state.tab = AppTab::Editor;
                                    }
                                } else if ext == "zip" {
                                    if let Err(e) = state.zip_debugger.extract_zip(&entry.path) {
                                        state.logs.push(format!("Failed to extract zip: {}", e));
                                    } else {
                                        state.logs.push("Zip extracted and analyzed.".into());
                                    }
                                } else {
                                    state.logs.push("Cannot open this file type.".into());
                                }
                            }
                        }
                    }
                    ui.label(format!("{}", entry.size));
                });
            }
        });
    }

    fn render_editor(&self, ui: &mut egui::Ui, state: &mut AixState) {
        ui.horizontal(|ui| {
            if let Some(path) = &state.editor.current_file {
                ui.label(format!("Editing: {}", path.display()));
            } else {
                ui.label("No file loaded");
            }
            if ui.button("Save").clicked() {
                if let Err(e) = state.editor.save_file() {
                    state.logs.push(format!("Save failed: {}", e));
                } else {
                    state.logs.push("File saved".into());
                }
            }
            if ui.button("Close").clicked() {
                state.editor.current_file = None;
                state.editor.content.clear();
                state.tab = AppTab::FileBrowser;
            }
        });
        ui.separator();

        // Simple text editor with scrolling
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut state.editor.content)
                    .desired_width(f32::INFINITY)
                    .font(egui::FontId::monospace(14.0)),
            );
        });
    }

    fn render_settings(&self, ui: &mut egui::Ui, state: &mut AixState) {
        ui.heading("Settings");
        ui.checkbox(&mut state.settings.dark_mode, "Dark Mode");
        ui.horizontal(|ui| {
            ui.label("Model Path:");
            let mut path_str = state.settings.model_path.display().to_string();
            if ui.add(egui::TextEdit::singleline(&mut path_str)).changed() {
                state.settings.model_path = PathBuf::from(path_str);
            }
        });
        if ui.button("Load Model").clicked() {
            state.load_model();
        }
        if state.model_loading {
            ui.spinner();
        }
        if let Some(err) = &state.model_error {
            ui.colored_label(egui::Color32::RED, format!("Error: {}", err));
        }
        if state.model.is_some() {
            ui.colored_label(egui::Color32::GREEN, "Model loaded");
        }
        ui.checkbox(&mut state.settings.auto_save, "Auto-save chat history");
        if ui.button("Save Settings").clicked() {
            // Persist settings (could save to file)
            state.logs.push("Settings saved".into());
        }
    }
}

impl eframe::App for AixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut state = self.state.lock().unwrap();

        // Apply theme
        let visuals = if state.settings.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        ctx.set_visuals(visuals);

        // Top panel: header
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("⚡ AIX ULTRA");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("🌙").clicked() {
                        state.settings.dark_mode = !state.settings.dark_mode;
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.tab, AppTab::Chat, "💬 CHAT");
                ui.selectable_value(&mut state.tab, AppTab::Shell, "🐚 SHELL");
                ui.selectable_value(&mut state.tab, AppTab::Hardware, "📊 SYS");
                ui.selectable_value(&mut state.tab, AppTab::FileBrowser, "📁 FILES");
                ui.selectable_value(&mut state.tab, AppTab::Editor, "✏️ EDITOR");
                ui.selectable_value(&mut state.tab, AppTab::Settings, "⚙️ SETTINGS");
            });
        });

        // Central panel: content
        egui::CentralPanel::default().show(ctx, |ui| {
            match state.tab {
                AppTab::Chat => self.render_chat(ui, &mut state),
                AppTab::Shell => self.render_shell(ui, &mut state),
                AppTab::Hardware => self.render_hardware(ui, &mut state),
                AppTab::FileBrowser => self.render_file_browser(ui, &mut state),
                AppTab::Editor => self.render_editor(ui, &mut state),
                AppTab::Settings => self.render_settings(ui, &mut state),
            }
        });

        // Bottom panel: input field
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let response = ui.add_sized(
                    [ui.available_width() - 70.0, 35.0],
                    egui::TextEdit::singleline(&mut state.input)
                        .hint_text("Type a message or shell command...")
                );
                if ui.button("SEND").clicked() || (response.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))) {
                    let text = state.input.clone();
                    if !text.is_empty() {
                        match state.tab {
                            AppTab::Chat => state.send_message(&text),
                            AppTab::Shell => {
                                let res = state.shell_command(&text);
                                state.logs.push(format!("$ {}", text));
                                state.logs.push(res);
                            }
                            _ => {
                                // Do nothing
                            }
                        }
                        state.input.clear();
                    }
                }
            });
        });
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(_app: android_activity::AndroidApp) {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "AIX Ultra",
        options,
        Box::new(|cc| Ok(Box::new(AixApp::new(cc)))),
    ).unwrap();
}
