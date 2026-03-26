//! AIX Ultra – Phase 2: Agentic AI with OpenAI‑compatible API, tools, and memory.

#![cfg_attr(target_os = "android", allow(unused_imports))]

use anyhow::{anyhow, Result};
use chrono::Local;
use directories::ProjectDirs;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, Duration};
use walkdir::WalkDir;
use zip::read::ZipArchive;

// Syntect for syntax highlighting
use syntect::easy::HighlightLines;
use syntect::highlighting::{ThemeSet, Style};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

// Logging
use log;

// Async
use tokio::runtime::Runtime;

// -----------------------------------------------------------------------------
// App tabs
// -----------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
enum AppTab {
    Welcome,
    Chat,
    FileBrowser,
    Editor,
    ZipDebugger,
    Notes,
    Tasks,
    Calculator,
    Hardware,
    Search,
    Settings,
}

// -----------------------------------------------------------------------------
// Chat message (for UI)
// -----------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
struct ChatMessage {
    sender: String,
    text: String,
    time: String,
}

// -----------------------------------------------------------------------------
// Agent core
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Action {
    tool: String,
    args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum ActionResult {
    Success(String),
    Failure(String),
    Data(Vec<u8>),
    None,
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    async fn execute(&self, args: &str) -> Result<ActionResult>;
}

// -----------------------------------------------------------------------------
// Tools
// -----------------------------------------------------------------------------

pub struct ShellTool;

#[async_trait::async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &'static str { "shell" }
    fn description(&self) -> &'static str { "Execute a shell command" }
    async fn execute(&self, args: &str) -> Result<ActionResult> {
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(args)
            .output()
            .await?;
        let result = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(ActionResult::Success(result))
    }
}

pub struct WebTool;

#[async_trait::async_trait]
impl Tool for WebTool {
    fn name(&self) -> &'static str { "web" }
    fn description(&self) -> &'static str { "Fetch a URL and return HTML content" }
    async fn execute(&self, args: &str) -> Result<ActionResult> {
        let client = reqwest::Client::new();
        let response = client.get(args).send().await?;
        let text = response.text().await?;
        Ok(ActionResult::Success(text))
    }
}

pub struct FileTool;

#[async_trait::async_trait]
impl Tool for FileTool {
    fn name(&self) -> &'static str { "file" }
    fn description(&self) -> &'static str { "Read, write, list, delete files (args: operation path [content])" }
    async fn execute(&self, args: &str) -> Result<ActionResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(ActionResult::Failure("Missing arguments".into()));
        }
        match parts[0] {
            "read" => {
                if parts.len() < 2 { return Ok(ActionResult::Failure("Missing path".into())); }
                let path = PathBuf::from(parts[1]);
                let content = fs::read_to_string(&path)?;
                Ok(ActionResult::Success(content))
            }
            "write" => {
                if parts.len() < 3 { return Ok(ActionResult::Failure("Missing path or content".into())); }
                let path = PathBuf::from(parts[1]);
                let content = parts[2..].join(" ");
                fs::write(&path, content)?;
                Ok(ActionResult::Success("File written".into()))
            }
            "list" => {
                let dir = if parts.len() > 1 { parts[1] } else { "." };
                let entries = fs::read_dir(dir)?;
                let mut out = String::new();
                for entry in entries {
                    let entry = entry?;
                    out.push_str(&entry.file_name().to_string_lossy());
                    out.push('\n');
                }
                Ok(ActionResult::Success(out))
            }
            "delete" => {
                if parts.len() < 2 { return Ok(ActionResult::Failure("Missing path".into())); }
                let path = PathBuf::from(parts[1]);
                fs::remove_file(&path)?;
                Ok(ActionResult::Success("File deleted".into()))
            }
            _ => Ok(ActionResult::Failure("Unknown file operation".into())),
        }
    }
}

pub struct CalculatorTool;

#[async_trait::async_trait]
impl Tool for CalculatorTool {
    fn name(&self) -> &'static str { "calc" }
    fn description(&self) -> &'static str { "Evaluate a mathematical expression" }
    async fn execute(&self, args: &str) -> Result<ActionResult> {
        let expr = args.trim();
        if expr.is_empty() {
            return Ok(ActionResult::Failure("Empty expression".into()));
        }
        let tokens: Vec<&str> = expr.split_whitespace().collect();
        let mut result = 0.0;
        let mut op = '+';
        for tok in tokens {
            if let Ok(num) = tok.parse::<f64>() {
                match op {
                    '+' => result += num,
                    '-' => result -= num,
                    '*' => result *= num,
                    '/' => result /= num,
                    _ => {}
                }
            } else {
                op = match tok {
                    "+" => '+',
                    "-" => '-',
                    "*" => '*',
                    "/" => '/',
                    _ => '?',
                };
            }
        }
        Ok(ActionResult::Success(format!("{}", result)))
    }
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut reg = Self { tools: HashMap::new() };
        reg.register(Box::new(ShellTool));
        reg.register(Box::new(WebTool));
        reg.register(Box::new(FileTool));
        reg.register(Box::new(CalculatorTool));
        reg
    }
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }
    pub fn get(&self, name: &str) -> Option<&Box<dyn Tool>> {
        self.tools.get(name)
    }
    pub async fn execute(&self, name: &str, args: &str) -> Result<ActionResult> {
        if let Some(tool) = self.get(name) {
            tool.execute(args).await
        } else {
            Err(anyhow!("Tool '{}' not found", name))
        }
    }
}

// -----------------------------------------------------------------------------
// LLM client (OpenAI‑compatible)
// -----------------------------------------------------------------------------

#[derive(Clone)]
pub struct LlmClient {
    api_key: String,
    base_url: String,
    model: String,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessageApi>,
    temperature: f32,
    max_tokens: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessageApi {
    role: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Choice {
    message: ChatMessageApi,
}

impl LlmClient {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            api_key,
            base_url,
            model,
            client: reqwest::Client::new(),
        }
    }

    pub async fn complete(&self, prompt: &str, system: &str) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url);
        let messages = vec![
            ChatMessageApi { role: "system".to_string(), content: system.to_string() },
            ChatMessageApi { role: "user".to_string(), content: prompt.to_string() },
        ];
        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            temperature: 0.7,
            max_tokens: 1024,
        };
        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;
        if !response.status().is_success() {
            let text = response.text().await?;
            return Err(anyhow!("LLM API error: {}", text));
        }
        let body: ChatResponse = response.json().await?;
        if let Some(choice) = body.choices.first() {
            Ok(choice.message.content.clone())
        } else {
            Err(anyhow!("No choices in response"))
        }
    }
}

// -----------------------------------------------------------------------------
// Agent
// -----------------------------------------------------------------------------

struct Agent {
    llm: LlmClient,
    tools: ToolRegistry,
    memory: Vec<String>, // simple memory for now
}

impl Agent {
    fn new(llm: LlmClient) -> Self {
        Self {
            llm,
            tools: ToolRegistry::new(),
            memory: Vec::new(),
        }
    }

    async fn plan(&mut self, goal: &str) -> Result<Vec<Action>> {
        let system = "You are a task planner. Given a goal, break it into a sequence of actions. Each action is a JSON object with fields 'tool' and 'args'. Available tools: shell, web, file, calc. Return only a JSON array of actions, nothing else.";
        let prompt = format!("Goal: {}", goal);
        let response = self.llm.complete(&prompt, system).await?;
        let actions: Vec<Action> = serde_json::from_str(&response)?;
        Ok(actions)
    }

    async fn execute_action(&self, action: &Action) -> Result<ActionResult> {
        self.tools.execute(&action.tool, &action.args).await
    }

    async fn run_agent(&mut self, goal: &str) -> Result<String> {
        let actions = self.plan(goal).await?;
        let mut outputs = Vec::new();
        for action in actions {
            let result = self.execute_action(&action).await?;
            match result {
                ActionResult::Success(s) => outputs.push(s),
                ActionResult::Failure(e) => outputs.push(format!("Error: {}", e)),
                _ => outputs.push("No output".into()),
            }
        }
        Ok(outputs.join("\n"))
    }
}

// -----------------------------------------------------------------------------
// Notes & Tasks
// -----------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
struct Note {
    title: String,
    content: String,
    updated: SystemTime,
}

#[derive(Serialize, Deserialize, Clone)]
struct Task {
    id: u64,
    text: String,
    completed: bool,
    created: SystemTime,
}

// -----------------------------------------------------------------------------
// Settings
// -----------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
struct Settings {
    dark_mode: bool,
    api_key: String,
    api_base_url: String,
    model: String,
    notes_file: PathBuf,
    tasks_file: PathBuf,
    chat_history_file: PathBuf,
}

impl Default for Settings {
    fn default() -> Self {
        let proj = ProjectDirs::from("com", "i7m7r8", "AIX").unwrap();
        let data_dir = proj.data_dir();
        fs::create_dir_all(&data_dir).ok();
        Self {
            dark_mode: true,
            api_key: String::new(),
            api_base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-3.5-turbo".to_string(),
            notes_file: data_dir.join("notes.json"),
            tasks_file: data_dir.join("tasks.json"),
            chat_history_file: data_dir.join("chat_history.json"),
        }
    }
}

// -----------------------------------------------------------------------------
// File browser, editor, zip debugger, search, calculator
// -----------------------------------------------------------------------------

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
    highlighter: Option<HighlightLines<'static>>,
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
                                if content.contains("unsafe") {
                                    self.warnings.push(format!("{}: contains unsafe code", path.display()));
                                }
                                if content.contains("TODO") {
                                    self.warnings.push(format!("{}: contains TODO", path.display()));
                                }
                                if content.contains("panic!") {
                                    self.errors.push(format!("{}: contains panic! macro", path.display()));
                                }
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

struct SearchState {
    query: String,
    results: Vec<PathBuf>,
    searching: bool,
}

impl SearchState {
    fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            searching: false,
        }
    }
    fn start_search(&mut self, root: &Path) {
        self.results.clear();
        self.searching = true;
        let query = self.query.clone();
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            let mut found = Vec::new();
            for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
                if entry.file_name().to_string_lossy().contains(&query) {
                    found.push(entry.path().to_path_buf());
                }
            }
            eprintln!("Found {} files", found.len());
        });
    }
}

struct CalculatorState {
    expression: String,
    result: String,
}

impl CalculatorState {
    fn new() -> Self {
        Self {
            expression: String::new(),
            result: String::new(),
        }
    }
    fn evaluate(&mut self) {
        let expr = self.expression.trim();
        if expr.is_empty() {
            self.result = "".into();
            return;
        }
        let tokens: Vec<&str> = expr.split_whitespace().collect();
        if tokens.is_empty() {
            self.result = "Invalid expression".into();
            return;
        }
        let mut result = 0.0;
        let mut op = '+';
        for tok in tokens {
            if let Ok(num) = tok.parse::<f64>() {
                match op {
                    '+' => result += num,
                    '-' => result -= num,
                    '*' => result *= num,
                    '/' => result /= num,
                    _ => {}
                }
            } else {
                op = match tok {
                    "+" => '+',
                    "-" => '-',
                    "*" => '*',
                    "/" => '/',
                    _ => '?',
                };
            }
        }
        self.result = format!("{}", result);
    }
}

// -----------------------------------------------------------------------------
// Main app state
// -----------------------------------------------------------------------------

struct AixState {
    tab: AppTab,
    chat_history: Vec<ChatMessage>,
    chat_input: String,
    agent: Option<Agent>,
    agent_running: bool,
    logs: Vec<String>,
    settings: Settings,
    file_browser_current_dir: PathBuf,
    file_entries: Vec<FileEntry>,
    zip_debugger: ZipDebugger,
    editor: EditorState,
    notes: Vec<Note>,
    tasks: Vec<Task>,
    next_task_id: u64,
    calculator: CalculatorState,
    search: SearchState,
    shell_input: String,
}

impl AixState {
    fn new() -> Self {
        let settings = Settings::default();
        let proj = ProjectDirs::from("com", "i7m7r8", "AIX").unwrap();
        let data_dir = proj.data_dir();
        fs::create_dir_all(&data_dir).ok();

        let notes = if let Ok(file) = File::open(&settings.notes_file) {
            serde_json::from_reader(file).unwrap_or_default()
        } else {
            Vec::new()
        };
        let tasks = if let Ok(file) = File::open(&settings.tasks_file) {
            serde_json::from_reader(file).unwrap_or_default()
        } else {
            Vec::new()
        };
        let max_id = tasks.iter().map(|t: &Task| t.id).max().unwrap_or(0) + 1;
        let chat_history = if let Ok(file) = File::open(&settings.chat_history_file) {
            serde_json::from_reader(file).unwrap_or_default()
        } else {
            Vec::new()
        };

        Self {
            tab: AppTab::Welcome,
            chat_history,
            chat_input: String::new(),
            agent: None,
            agent_running: false,
            logs: vec!["[BOOT] AIX Ultra started.".into()],
            settings,
            file_browser_current_dir: PathBuf::from("/sdcard"),
            file_entries: Vec::new(),
            zip_debugger: ZipDebugger::new(),
            editor: EditorState::new(),
            notes,
            tasks,
            next_task_id: max_id,
            calculator: CalculatorState::new(),
            search: SearchState::new(),
            shell_input: String::new(),
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

    fn init_agent(&mut self) -> bool {
        if self.agent.is_some() {
            return true;
        }
        if self.settings.api_key.is_empty() {
            self.logs.push("API key not set. Please configure in Settings.".into());
            return false;
        }
        let llm = LlmClient::new(
            self.settings.api_key.clone(),
            self.settings.api_base_url.clone(),
            self.settings.model.clone(),
        );
        self.agent = Some(Agent::new(llm));
        self.logs.push("Agent initialized with LLM.".into());
        true
    }

    fn send_chat_message(&mut self, text: &str) {
        if !self.init_agent() {
            self.chat_history.push(ChatMessage {
                sender: "AI".into(),
                text: "Please set your API key in Settings first.".into(),
                time: Local::now().format("%H:%M").to_string(),
            });
            return;
        }

        self.chat_history.push(ChatMessage {
            sender: "USER".into(),
            text: text.to_string(),
            time: Local::now().format("%H:%M").to_string(),
        });
        self.chat_input.clear();

        let loading_idx = self.chat_history.len();
        self.chat_history.push(ChatMessage {
            sender: "AI".into(),
            text: "Thinking...".into(),
            time: Local::now().format("%H:%M").to_string(),
        });

        self.agent_running = true;
        let agent = self.agent.as_mut().unwrap();
        let goal = text.to_string();
        // Run agent in a separate thread to avoid UI freeze
        // We'll use a blocking approach with a new runtime
        let runtime = Runtime::new().unwrap();
        let result = runtime.block_on(agent.run_agent(&goal));
        match result {
            Ok(response) => {
                self.chat_history[loading_idx] = ChatMessage {
                    sender: "AI".into(),
                    text: response,
                    time: Local::now().format("%H:%M").to_string(),
                };
            }
            Err(e) => {
                self.chat_history[loading_idx] = ChatMessage {
                    sender: "AI".into(),
                    text: format!("Error: {}", e),
                    time: Local::now().format("%H:%M").to_string(),
                };
            }
        }
        self.agent_running = false;

        let _ = fs::write(&self.settings.chat_history_file, serde_json::to_string_pretty(&self.chat_history).unwrap());
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

    fn add_note(&mut self, title: String, content: String) {
        self.notes.push(Note {
            title,
            content,
            updated: SystemTime::now(),
        });
        self.save_notes();
    }

    fn delete_note(&mut self, idx: usize) {
        self.notes.remove(idx);
        self.save_notes();
    }

    fn add_task(&mut self, text: String) {
        self.tasks.push(Task {
            id: self.next_task_id,
            text,
            completed: false,
            created: SystemTime::now(),
        });
        self.next_task_id += 1;
        self.save_tasks();
    }

    fn delete_task(&mut self, idx: usize) {
        self.tasks.remove(idx);
        self.save_tasks();
    }

    fn save_notes(&self) {
        let _ = fs::write(&self.settings.notes_file, serde_json::to_string_pretty(&self.notes).unwrap());
    }

    fn save_tasks(&self) {
        let _ = fs::write(&self.settings.tasks_file, serde_json::to_string_pretty(&self.tasks).unwrap());
    }
}

// -----------------------------------------------------------------------------
// App wrapper
// -----------------------------------------------------------------------------

struct AixApp {
    state: Arc<Mutex<AixState>>,
}

impl AixApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        android_logger::init_once(android_logger::Config::default().with_tag("AIX"));
        let state = AixState::new();
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    // -------------------------------------------------------------------------
    // Render functions
    // -------------------------------------------------------------------------

    fn render_welcome(&self, ui: &mut egui::Ui, _state: &mut AixState) {
        let welcome_text = include_str!("../assets/welcome.txt");
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(20.0);
            ui.heading("📱 Welcome to AIX Ultra");
            ui.add_space(10.0);
            ui.label(welcome_text);
            ui.add_space(20.0);
            ui.label("AI Agent uses OpenAI‑compatible API. Set your API key in Settings.");
        });
    }

    fn render_chat(&self, ui: &mut egui::Ui, state: &mut AixState) {
        egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
            for msg in &state.chat_history {
                ui.group(|ui| {
                    ui.label(format!("[{}] {}", msg.time, msg.sender));
                    ui.label(&msg.text);
                });
            }
        });
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

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut state.editor.content)
                    .desired_width(f32::INFINITY)
                    .font(egui::FontId::monospace(14.0)),
            );
        });
    }

    fn render_zip_debugger(&self, ui: &mut egui::Ui, state: &mut AixState) {
        if state.zip_debugger.extracted_dir.is_none() {
            ui.label("No zip extracted yet. Open a zip file from the file browser.");
        } else {
            ui.label("Extracted directory:");
            ui.monospace(state.zip_debugger.extracted_dir.as_ref().unwrap().display().to_string());
            ui.separator();
            ui.label("Analysis:");
            for line in &state.zip_debugger.analysis {
                ui.label(line);
            }
            ui.separator();
            ui.colored_label(egui::Color32::YELLOW, "Warnings:");
            for warn in &state.zip_debugger.warnings {
                ui.colored_label(egui::Color32::YELLOW, warn);
            }
            ui.separator();
            ui.colored_label(egui::Color32::RED, "Errors:");
            for err in &state.zip_debugger.errors {
                ui.colored_label(egui::Color32::RED, err);
            }
            if ui.button("Cleanup").clicked() {
                state.zip_debugger.cleanup();
            }
        }
    }

    fn render_notes(&self, ui: &mut egui::Ui, state: &mut AixState) {
        egui::SidePanel::left("notes_list").show_inside(ui, |ui| {
            ui.heading("Notes");
            let mut to_delete = Vec::new();
            for (i, note) in state.notes.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.button(&note.title).clicked() {}
                    if ui.button("❌").clicked() {
                        to_delete.push(i);
                    }
                });
            }
            for i in to_delete.into_iter().rev() {
                state.delete_note(i);
            }
            if ui.button("+ New Note").clicked() {
                state.add_note("New Note".into(), "Write your note here...".into());
            }
        });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(note) = state.notes.last() {
                ui.heading(&note.title);
                ui.label(&note.content);
            } else {
                ui.label("No notes. Click + to create one.");
            }
        });
    }

    fn render_tasks(&self, ui: &mut egui::Ui, state: &mut AixState) {
        ui.heading("To‑Do List");
        let mut to_delete = Vec::new();
        for (i, task) in state.tasks.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let mut completed = task.completed;
                if ui.checkbox(&mut completed, &task.text).changed() {
                    task.completed = completed;
                }
                if ui.button("❌").clicked() {
                    to_delete.push(i);
                }
            });
        }
        for i in to_delete.into_iter().rev() {
            state.delete_task(i);
        }
        state.save_tasks();
        ui.separator();
        ui.horizontal(|ui| {
            let _new_task = ui.text_edit_singleline(&mut state.shell_input);
            if ui.button("Add Task").clicked() {
                let text = state.shell_input.clone();
                if !text.is_empty() {
                    state.add_task(text);
                    state.shell_input.clear();
                }
            }
        });
    }

    fn render_calculator(&self, ui: &mut egui::Ui, state: &mut AixState) {
        ui.heading("Calculator");
        ui.label("Expression:");
        let response = ui.text_edit_singleline(&mut state.calculator.expression);
        if ui.button("=").clicked() || response.lost_focus() {
            state.calculator.evaluate();
        }
        ui.label(format!("Result: {}", state.calculator.result));
    }

    fn render_hardware(&self, ui: &mut egui::Ui, state: &mut AixState) {
        ui.label(state.hardware_info());
        if ui.button("Refresh").clicked() {}
    }

    fn render_search(&self, ui: &mut egui::Ui, state: &mut AixState) {
        ui.heading("File Search");
        ui.horizontal(|ui| {
            ui.label("Query:");
            let _response = ui.text_edit_singleline(&mut state.search.query);
            if ui.button("Search").clicked() {
                state.search.start_search(&state.file_browser_current_dir);
            }
        });
        ui.separator();
        ui.label("Results (demo – not fully implemented)");
        for path in &state.search.results {
            ui.label(path.display().to_string());
        }
    }

    fn render_settings(&self, ui: &mut egui::Ui, state: &mut AixState) {
        ui.heading("Settings");
        ui.checkbox(&mut state.settings.dark_mode, "Dark Mode");
        ui.horizontal(|ui| {
            ui.label("API Key:");
            ui.text_edit_singleline(&mut state.settings.api_key);
        });
        ui.horizontal(|ui| {
            ui.label("Base URL:");
            ui.text_edit_singleline(&mut state.settings.api_base_url);
        });
        ui.horizontal(|ui| {
            ui.label("Model:");
            ui.text_edit_singleline(&mut state.settings.model);
        });
        if ui.button("Save Settings").clicked() {
            state.agent = None;
            state.init_agent();
            state.logs.push("Settings saved and agent reinitialized.".into());
        }
        if ui.button("Test API").clicked() {
            if state.settings.api_key.is_empty() {
                state.logs.push("API key is empty".into());
            } else {
                let llm = LlmClient::new(
                    state.settings.api_key.clone(),
                    state.settings.api_base_url.clone(),
                    state.settings.model.clone(),
                );
                let runtime = Runtime::new().unwrap();
                let response = runtime.block_on(llm.complete("Hello, are you there?", "You are a helpful assistant."));
                match response {
                    Ok(msg) => state.logs.push(format!("Test OK: {}", msg)),
                    Err(e) => state.logs.push(format!("Test failed: {}", e)),
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Main update
    // -------------------------------------------------------------------------
}

impl eframe::App for AixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut state = self.state.lock().unwrap();

        // Material 3 theme
        let mut visuals = if state.settings.dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(30, 30, 35);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(40, 40, 45);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(60, 60, 70);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(80, 80, 90);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 200, 210));
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(210, 210, 220));
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(230, 230, 240));
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(255, 255, 255));
        visuals.widgets.noninteractive.rounding = 8.0.into();
        visuals.widgets.inactive.rounding = 8.0.into();
        visuals.widgets.hovered.rounding = 8.0.into();
        visuals.widgets.active.rounding = 8.0.into();
        visuals.window_rounding = 12.0.into();
        visuals.menu_rounding = 8.0.into();
        visuals.panel_fill = egui::Color32::from_rgb(25, 25, 30);
        visuals.window_fill = egui::Color32::from_rgb(35, 35, 40);
        visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 80, 90));
        ctx.set_visuals(visuals);

        // Top panel
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.style_mut().spacing.item_spacing = egui::vec2(8.0, 8.0);
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
                ui.selectable_value(&mut state.tab, AppTab::Welcome, "👋 WELCOME");
                ui.selectable_value(&mut state.tab, AppTab::Chat, "💬 CHAT");
                ui.selectable_value(&mut state.tab, AppTab::FileBrowser, "📁 FILES");
                ui.selectable_value(&mut state.tab, AppTab::Editor, "✏️ EDITOR");
                ui.selectable_value(&mut state.tab, AppTab::ZipDebugger, "📦 ZIP");
                ui.selectable_value(&mut state.tab, AppTab::Notes, "📝 NOTES");
                ui.selectable_value(&mut state.tab, AppTab::Tasks, "✅ TASKS");
                ui.selectable_value(&mut state.tab, AppTab::Calculator, "🔢 CALC");
                ui.selectable_value(&mut state.tab, AppTab::Hardware, "📊 SYS");
                ui.selectable_value(&mut state.tab, AppTab::Search, "🔍 SEARCH");
                ui.selectable_value(&mut state.tab, AppTab::Settings, "⚙️ SETTINGS");
            });
        });

        // Central panel
        egui::CentralPanel::default().show(ctx, |ui| {
            match state.tab {
                AppTab::Welcome => self.render_welcome(ui, &mut state),
                AppTab::Chat => self.render_chat(ui, &mut state),
                AppTab::FileBrowser => self.render_file_browser(ui, &mut state),
                AppTab::Editor => self.render_editor(ui, &mut state),
                AppTab::ZipDebugger => self.render_zip_debugger(ui, &mut state),
                AppTab::Notes => self.render_notes(ui, &mut state),
                AppTab::Tasks => self.render_tasks(ui, &mut state),
                AppTab::Calculator => self.render_calculator(ui, &mut state),
                AppTab::Hardware => self.render_hardware(ui, &mut state),
                AppTab::Search => self.render_search(ui, &mut state),
                AppTab::Settings => self.render_settings(ui, &mut state),
            }
        });

        // Bottom panel (only for chat input)
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            if state.tab == AppTab::Chat {
                ui.horizontal(|ui| {
                    let response = ui.add_sized(
                        [ui.available_width() - 70.0, 35.0],
                        egui::TextEdit::singleline(&mut state.chat_input).hint_text("Type a message..."),
                    );
                    if ui.button("SEND").clicked() || (response.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))) {
                        let text = state.chat_input.clone();
                        if !text.is_empty() {
                            state.send_chat_message(&text);
                        }
                    }
                });
            }
        });
    }
}

// Panic hook and Android entry point
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(_app: android_activity::AndroidApp) {
    std::panic::set_hook(Box::new(|info| {
        let log_path = "/sdcard/aix_crash.log";
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .and_then(|mut f| writeln!(f, "Panic at {:?}: {}", Local::now(), info));
        android_logger::init_once(android_logger::Config::default().with_tag("AIX"));
        log::error!("Panic: {}", info);
    }));
    let _ = std::fs::write("/sdcard/aix_startup.txt", "started");
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "AIX Ultra",
        options,
        Box::new(|cc| Ok(Box::new(AixApp::new(cc)))),
    ).unwrap();
}
