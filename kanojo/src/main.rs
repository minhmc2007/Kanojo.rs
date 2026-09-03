use anyhow::Result;
use clap::{Parser, Subcommand};
use kanojo_config::{Config, config_path};
use std::io::{self, Write};

#[derive(Parser)]
#[command(name = "kanojo", about = "Kanojo companion system")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Connect,
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonAction>,
    },
    Gateway,
}

#[derive(Subcommand)]
enum DaemonAction {
    Start,
    Stop,
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Init) => cmd_init()?,
        Some(Commands::Connect) => cmd_connect()?,
        Some(Commands::Daemon { action }) => cmd_daemon(action)?,
        Some(Commands::Gateway) => cmd_gateway()?,
        None => cmd_tui()?,
    }
    Ok(())
}

fn cmd_init() -> Result<()> {
    let mut config = Config::default();

    print!("Your name: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if !trimmed.is_empty() {
        config.user.name = trimmed.to_string();
    }

    print!("Companion name [{}]: ", config.companion.name);
    io::stdout().flush()?;
    input.clear();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if !trimmed.is_empty() {
        config.companion.name = trimmed.to_string();
    }

    print!("Persona [{}]: ", config.companion.persona);
    io::stdout().flush()?;
    input.clear();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if !trimmed.is_empty() {
        config.companion.persona = trimmed.to_string();
    }

    config.save()?;
    println!("Config saved to {}", config_path().display());
    Ok(())
}

fn cmd_connect() -> Result<()> {
    let mut config = Config::load()?;

    print!("LLM base URL [{}]: ", config.llm.base_url);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if !trimmed.is_empty() {
        config.llm.base_url = trimmed.to_string();
    }

    print!("Model [{}]: ", config.llm.model);
    io::stdout().flush()?;
    input.clear();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if !trimmed.is_empty() {
        config.llm.model = trimmed.to_string();
    }

    print!("API key (optional): ");
    io::stdout().flush()?;
    input.clear();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if !trimmed.is_empty() {
        config.llm.api_key = Some(trimmed.to_string());
    }

    config.save()?;
    println!("LLM config saved.");
    Ok(())
}

fn cmd_daemon(action: Option<DaemonAction>) -> Result<()> {
    match action {
        Some(DaemonAction::Start) | None => {
            println!("Starting kanojod...");
            std::process::Command::new("kanojod")
                .spawn()
                .map_err(|e| anyhow::anyhow!("failed to start kanojod: {}", e))?;
            println!("kanojod started.");
        }
        Some(DaemonAction::Stop) => {
            println!("Stopping kanojod...");
            std::process::Command::new("pkill")
                .arg("kanojod")
                .output()?;
            println!("kanojod stopped.");
        }
        Some(DaemonAction::Status) => {
            let output = std::process::Command::new("pgrep")
                .arg("-x")
                .arg("kanojod")
                .output()?;
            if output.status.success() {
                println!(
                    "kanojod is running (pid: {})",
                    String::from_utf8_lossy(&output.stdout).trim()
                );
            } else {
                println!("kanojod is not running.");
            }
        }
    }
    Ok(())
}

fn cmd_gateway() -> Result<()> {
    let mut config = Config::load()?;

    print!("Telegram bot token: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        println!("Bot token required.");
        return Ok(());
    }
    let token = trimmed.to_string();

    print!("Allowed chat IDs (comma-separated): ");
    io::stdout().flush()?;
    input.clear();
    io::stdin().read_line(&mut input)?;
    let chat_ids: Vec<i64> = input
        .trim()
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    config.telegram = Some(kanojo_config::TelegramConfig {
        bot_token: token,
        allowed_chat_ids: chat_ids,
    });
    config.save()?;
    println!("Telegram gateway configured.");
    Ok(())
}

// --- Ratatui TUI ---

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
enum ChatEntry {
    User(String),
    Companion {
        action: Option<String>,
        dialogue: String,
        thought: Option<String>,
    },
    #[allow(dead_code)]
    System(String),
    Typing,
}

struct TuiState {
    entries: VecDeque<ChatEntry>,
    input: String,
    scroll_offset: usize,
    is_waiting: bool,
    emotion: String,
    activity: String,
    error: Option<String>,
}

impl TuiState {
    fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            input: String::new(),
            scroll_offset: 0,
            is_waiting: false,
            emotion: "Casual".into(),
            activity: "Idle".into(),
            error: None,
        }
    }

    fn push_entry(&mut self, entry: ChatEntry) {
        self.entries.push_back(entry);
        // Auto-scroll to bottom
        self.scroll_offset = self.entries.len();
    }

    fn max_scroll(&self) -> usize {
        self.entries.len()
    }
}

fn emotion_color(emotion: &str) -> Color {
    match emotion {
        "InLove" => Color::Magenta,
        "Affectionate" => Color::LightMagenta,
        "Casual" => Color::Cyan,
        "Annoyed" => Color::Yellow,
        "Angry" => Color::Red,
        "Furious" => Color::LightRed,
        "Distant" => Color::DarkGray,
        _ => Color::White,
    }
}

fn draw_ui(f: &mut Frame, state: &TuiState, config: &Config) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Status bar
            Constraint::Min(1),   // Chat area
            Constraint::Length(3), // Input box
        ])
        .split(f.area());

    draw_status_bar(f, chunks[0], state, config);
    draw_chat_area(f, chunks[1], state, config);
    draw_input_box(f, chunks[2], state, config);
}

fn draw_status_bar(f: &mut Frame, area: Rect, state: &TuiState, config: &Config) {
    let emotion_color = emotion_color(&state.emotion);
    let status_line = Line::from(vec![
        Span::styled(
            format!(" {} ", config.companion.name),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("{} ", state.emotion),
            Style::default()
                .fg(emotion_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("{}", state.activity),
            Style::default().fg(Color::Gray),
        ),
        Span::raw(" | "),
        Span::styled(
            format!("{} ", config.user.name),
            Style::default().fg(Color::Blue),
        ),
    ]);

    let status = Paragraph::new(status_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(status, area);
}

fn draw_chat_area(f: &mut Frame, area: Rect, state: &TuiState, config: &Config) {
    let mut lines: Vec<Line> = Vec::new();

    for entry in &state.entries {
        match entry {
            ChatEntry::User(text) => {
                lines.push(Line::from(Span::styled(
                    format!("{}> {}", config.user.name, text),
                    Style::default().fg(Color::Blue),
                )));
            }
            ChatEntry::Companion {
                action,
                dialogue,
                thought,
            } => {
                if let Some(a) = action {
                    lines.push(Line::from(Span::styled(
                        format!("  *{}*", a),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    )));
                }
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {}: ", config.companion.name),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(dialogue.clone()),
                ]));
                if let Some(t) = thought {
                    lines.push(Line::from(Span::styled(
                        format!("    (thought: {})", t),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::ITALIC),
                    )));
                }
            }
            ChatEntry::System(text) => {
                lines.push(Line::from(Span::styled(
                    format!("  [system] {}", text),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            ChatEntry::Typing => {
                lines.push(Line::from(Span::styled(
                    "  ...",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::SLOW_BLINK),
                )));
            }
        }
    }

    // Error display
    if let Some(err) = &state.error {
        lines.push(Line::from(Span::styled(
            format!("  [error] {}", err),
            Style::default().fg(Color::Red),
        )));
    }

    let chat = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Chat ")
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(chat, area);
}

fn draw_input_box(f: &mut Frame, area: Rect, state: &TuiState, _config: &Config) {
    let input_line = Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Green)),
        Span::raw(&state.input),
    ]);

    let input = Paragraph::new(input_line).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Input (Enter=send, Esc=quit) ")
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(input, area);

    // Show cursor
    f.set_cursor_position((area.x + 2 + state.input.len() as u16, area.y + 1));
}

fn cmd_tui() -> Result<()> {
    let config = Config::load()?;
    let socket_path = std::path::PathBuf::from(&config.daemon.socket_path);

    // Auto-start daemon if socket doesn't exist
    if !socket_path.exists() {
        println!("Daemon not running. Starting kanojod...");
        let kanojod_bin = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("kanojod")))
            .filter(|p| p.exists())
            .unwrap_or_else(|| "kanojod".into());
        std::process::Command::new(&kanojod_bin)
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to start kanojod ({}): {}. Run 'kanojod' manually.",
                    kanojod_bin.display(),
                    e
                )
            })?;
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(300));
            if socket_path.exists() {
                break;
            }
        }
        if !socket_path.exists() {
            return Err(anyhow::anyhow!(
                "daemon started but socket not found at {}",
                socket_path.display()
            ));
        }
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = TuiState::new();

    let rt = tokio::runtime::Runtime::new()?;

    // Bidirectional daemon channel: send requests, receive responses
    let (req_tx, mut req_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, String)>();
    let (resp_tx, mut resp_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    rt.spawn(async move {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixStream;

        let stream = match UnixStream::connect(&socket_path).await {
            Ok(s) => s,
            Err(e) => {
                let _ = resp_tx.send(format!("__error__:{}", e));
                return;
            }
        };
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let _ = resp_tx.send("__connected__".to_string());

        loop {
            tokio::select! {
                Some((id, text)) = req_rx.recv() => {
                    let req = kanojod_ipc::JsonRpcRequest::send_message(id, "terminal", &text);
                    if let Ok(json) = serde_json::to_string(&req) {
                        let _ = writer.write_all(json.as_bytes()).await;
                        let _ = writer.write_all(b"\n").await;
                    }
                }
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            let _ = resp_tx.send(line);
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }
        }
    });

    // Wait for connection
    let mut connected = false;
    for _ in 0..50 {
        if let Ok(msg) = resp_rx.try_recv() {
            if msg == "__connected__" {
                connected = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if !connected {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        return Err(anyhow::anyhow!("failed to connect to daemon"));
    }

    let mut msg_id: u64 = 1;

    loop {
    terminal.draw(|f| draw_ui(f, &state, &config))?;

        // Handle terminal events
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Esc => break,
                    KeyCode::Enter => {
                        let text = state.input.trim().to_string();
                        if text.is_empty() || state.is_waiting {
                            continue;
                        }
                        state.push_entry(ChatEntry::User(text.clone()));
                        state.push_entry(ChatEntry::Typing);
                        state.is_waiting = true;
                        state.error = None;
                        let _ = req_tx.send((msg_id, text));
                        msg_id += 1;
                    }
                    KeyCode::Char(c) => {
                        if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) && c == 'c' {
                            state.input.clear();
                        } else {
                            state.input.push(c);
                        }
                    }
                    KeyCode::Backspace => {
                        state.input.pop();
                    }
                    KeyCode::Up => {
                        if state.scroll_offset > 0 {
                            state.scroll_offset -= 1;
                        }
                    }
                    KeyCode::Down => {
                        if state.scroll_offset < state.max_scroll() {
                            state.scroll_offset += 1;
                        }
                    }
                    KeyCode::PageUp => {
                        state.scroll_offset = state.scroll_offset.saturating_sub(10);
                    }
                    KeyCode::PageDown => {
                        state.scroll_offset =
                            (state.scroll_offset + 10).min(state.max_scroll());
                    }
                    _ => {}
                }
            }
        }

        // Handle daemon responses
        while let Ok(msg) = resp_rx.try_recv() {
            if msg == "__connected__" || msg.starts_with("__error__") {
                continue;
            }
            // Remove typing indicator
            state.entries.retain(|e| !matches!(e, ChatEntry::Typing));
            state.is_waiting = false;

            if let Ok(resp) = serde_json::from_str::<kanojod_ipc::JsonRpcResponse>(&msg) {
                if let Some(err) = resp.result["error"].as_str() {
                    state.error = Some(err.to_string());
                } else {
                    let action = resp.result["action_text"]
                        .as_str()
                        .map(String::from);
                    let dialogue = resp.result["dialogue"]
                        .as_str()
                        .unwrap_or("...")
                        .to_string();
                    let thought = resp.result["inner_thought"]
                        .as_str()
                        .map(String::from);
                    if let Some(e) = resp.result["emotion"].as_str() {
                        state.emotion = e.to_string();
                    }
                    state.push_entry(ChatEntry::Companion {
                        action,
                        dialogue,
                        thought,
                    });
                }
            }
        }
    }

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
