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

fn prompt_with_default(label: &str, current: &str) -> Result<String> {
    if current.is_empty() {
        print!("{}: ", label);
    } else {
        print!("{} [{}]: ", label, current);
    }
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    Ok(if trimmed.is_empty() {
        current.to_string()
    } else {
        trimmed.to_string()
    })
}

fn cmd_init() -> Result<()> {
    let mut config = Config::default();

    let val = prompt_with_default("Your name", &config.user.name)?;
    config.user.name = val;

    let val = prompt_with_default("Companion name", &config.companion.name)?;
    config.companion.name = val;

    let val = prompt_with_default("Persona", &config.companion.persona)?;
    config.companion.persona = val;

    config.save()?;
    println!("Config saved to {}", config_path().display());
    Ok(())
}

fn cmd_connect() -> Result<()> {
    let config = Config::load().unwrap_or_default();

    let val = prompt_with_default("LLM base URL", &config.llm.base_url)?;
    let mut new_config = config.clone();
    new_config.llm.base_url = val;

    let val = prompt_with_default("Model", &config.llm.model)?;
    new_config.llm.model = val;

    let current_key = config.llm.api_key.as_deref().unwrap_or("");
    let val = prompt_with_default("API key (optional)", current_key)?;
    new_config.llm.api_key = if val.is_empty() { None } else { Some(val) };

    new_config.save()?;
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
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame, Terminal,
};
use std::collections::VecDeque;

const COMMANDS: &[(&str, &str)] = &[
    ("/exit", "quit TUI"),
    ("/new", "clear session"),
    ("/db", "show database"),
    ("/debug", "show debug info"),
];

#[derive(Clone, Debug)]
enum ChatEntry {
    User(String),
    Companion {
        action: Option<String>,
        dialogue: String,
        thought: Option<String>,
    },
    System(String),
    Typing,
}

struct TuiState {
    entries: VecDeque<ChatEntry>,
    input: String,
    cursor_pos: usize,
    scroll_offset: usize,
    is_waiting: bool,
    emotion: String,
    activity: String,
    error: Option<String>,
    show_debug: bool,
    suggestions: Vec<(&'static str, &'static str)>,
    suggestion_idx: usize,
}

impl TuiState {
    fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            input: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            is_waiting: false,
            emotion: "Casual".into(),
            activity: "Idle".into(),
            error: None,
            show_debug: false,
            suggestions: Vec::new(),
            suggestion_idx: 0,
        }
    }

    fn push_entry(&mut self, entry: ChatEntry) {
        self.entries.push_back(entry);
        self.scroll_offset = self.entries.len();
    }

    fn max_scroll(&self) -> usize {
        self.entries.len()
    }

    fn update_suggestions(&mut self) {
        if self.input.starts_with('/') && !self.input.contains(' ') {
            let prefix = &self.input[1..];
            self.suggestions = COMMANDS
                .iter()
                .filter(|(cmd, _)| cmd[1..].starts_with(prefix))
                .copied()
                .collect();
            self.suggestion_idx = 0;
        } else {
            self.suggestions.clear();
            self.suggestion_idx = 0;
        }
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
    let mut constraints = vec![
        Constraint::Length(3),
        Constraint::Min(1),
    ];

    if !state.suggestions.is_empty() {
        let sug_lines = state.suggestions.len() as u16 + 2;
        constraints.push(Constraint::Length(sug_lines));
    }

    constraints.push(Constraint::Length(3));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(f.area());

    let mut idx = 0;
    draw_status_bar(f, chunks[idx], state, config);
    idx += 1;
    draw_chat_area(f, chunks[idx], state, config);
    idx += 1;

    if !state.suggestions.is_empty() {
        draw_suggestions(f, chunks[idx], state);
        idx += 1;
    }

    draw_input_box(f, chunks[idx], state);
}

fn draw_status_bar(f: &mut Frame, area: Rect, state: &TuiState, config: &Config) {
    let ec = emotion_color(&state.emotion);
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
            Style::default().fg(ec).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" | "),
        Span::styled(
            state.activity.to_string(),
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

fn draw_suggestions(f: &mut Frame, area: Rect, state: &TuiState) {
    let mut lines: Vec<Line> = Vec::new();
    for (i, &(cmd, desc)) in state.suggestions.iter().enumerate() {
        let style = if i == state.suggestion_idx {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {:12}", cmd), style),
            Span::styled(
                desc.to_string(),
                if i == state.suggestion_idx {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]));
    }

    let sug = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Commands ")
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(sug, area);
}

fn draw_input_box(f: &mut Frame, area: Rect, state: &TuiState) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let prefix = "> ";
    let prefix_len = prefix.len();
    let visible_width = inner_width.saturating_sub(prefix_len);

    let visible_pos = state.cursor_pos.min(state.input.len());
    let scroll = visible_pos.saturating_sub(visible_width);
    let vis_start = scroll;
    let vis_end = (scroll + visible_width).min(state.input.len());
    let visible_input = &state.input[vis_start..vis_end];

    let input_line = Line::from(vec![
        Span::styled(prefix, Style::default().fg(Color::Green)),
        Span::raw(visible_input),
    ]);

    let is_cmd = state.input.starts_with('/') && !state.suggestions.is_empty();
    let title = if state.is_waiting {
        " Waiting... "
    } else if is_cmd {
        " Tab=complete Enter=execute Esc=cancel "
    } else {
        " Enter=send Tab=complete Esc=cancel "
    };

    let input = Paragraph::new(input_line).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(input, area);

    let cursor_visual = state.cursor_pos.saturating_sub(scroll);
    let cursor_x = area.x + 1 + prefix_len as u16 + cursor_visual as u16;
    let cursor_y = area.y + 1;
    f.set_cursor_position((cursor_x, cursor_y));
}

fn cmd_tui() -> Result<()> {
    let config = Config::load()?;
    let socket_path = std::path::PathBuf::from(&config.daemon.socket_path);

    if !socket_path.exists() {
        println!("Daemon not running. Starting kanojod...");
        let kanojod_bin = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("kanojod")))
            .filter(|p| p.exists())
            .unwrap_or_else(|| "kanojod".into());
        let devnull = std::fs::File::open(std::path::Path::new("/dev/null"))?;
        std::process::Command::new(&kanojod_bin)
            .stdout(devnull.try_clone()?)
            .stderr(devnull)
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
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = TuiState::new();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;

    let (req_tx, mut req_rx) = tokio::sync::mpsc::unbounded_channel::<kanojod_ipc::JsonRpcRequest>();
    let (resp_tx, mut resp_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let sock = socket_path.clone();

    rt.spawn(async move {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixStream;

        let stream = match UnixStream::connect(&sock).await {
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
                Some(req) = req_rx.recv() => {
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

    let mut connected = false;
    for _ in 0..100 {
        if let Ok(msg) = resp_rx.try_recv() {
            if msg == "__connected__" {
                connected = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if !connected {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        return Err(anyhow::anyhow!(
            "failed to connect to daemon at {}",
            socket_path.display()
        ));
    }

    let mut msg_id: u64 = 1;

    loop {
        terminal.draw(|f| draw_ui(f, &state, &config))?;

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Esc => {
                        if state.is_waiting {
                            state.is_waiting = false;
                            state.entries.retain(|e| !matches!(e, ChatEntry::Typing));
                        } else {
                            state.input.clear();
                            state.cursor_pos = 0;
                            state.suggestions.clear();
                        }
                    }
                    KeyCode::Tab => {
                        if !state.suggestions.is_empty() {
                            let (cmd, _) = state.suggestions[state.suggestion_idx];
                            state.input.clear();
                            state.input.push_str(cmd);
                            state.input.push(' ');
                            state.cursor_pos = state.input.len();
                            state.suggestions.clear();
                        }
                    }
                    KeyCode::Enter => {
                        let text = state.input.trim().to_string();
                        if text.is_empty() || state.is_waiting {
                            continue;
                        }

                        if text.starts_with('/') {
                            let cmd_name = text.split_whitespace().next().unwrap_or("");
                            match cmd_name {
                                "/exit" | "/quit" => break,
                                "/new" => {
                                    state.entries.clear();
                                    state.error = None;
                                    state.scroll_offset = 0;
                                    state.push_entry(ChatEntry::System(
                                        "Session cleared.".into(),
                                    ));
                                }
                                "/db" => {
                                    let req = kanojod_ipc::JsonRpcRequest::db_query(msg_id);
                                    let _ = req_tx.send(req);
                                    msg_id += 1;
                                }
                                "/debug" => {
                                    let req = kanojod_ipc::JsonRpcRequest::debug_info(msg_id);
                                    let _ = req_tx.send(req);
                                    msg_id += 1;
                                }
                                _ => {
                                    state.error = Some(format!("command not found: {}", cmd_name));
                                }
                            }
                            state.input.clear();
                            state.cursor_pos = 0;
                            state.suggestions.clear();
                            continue;
                        }

                        state.push_entry(ChatEntry::User(text.clone()));
                        state.push_entry(ChatEntry::Typing);
                        state.is_waiting = true;
                        state.error = None;
                        state.show_debug = false;
                        state.input.clear();
                        state.cursor_pos = 0;
                        state.suggestions.clear();
                        let req = kanojod_ipc::JsonRpcRequest::send_message(msg_id, "terminal", &text);
                        let _ = req_tx.send(req);
                        msg_id += 1;
                    }
                    KeyCode::Char(c) => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                            state.input.clear();
                            state.cursor_pos = 0;
                        } else {
                            state.input.insert(state.cursor_pos, c);
                            state.cursor_pos += 1;
                        }
                        state.update_suggestions();
                    }
                    KeyCode::Backspace => {
                        if state.cursor_pos > 0 {
                            state.cursor_pos -= 1;
                            state.input.remove(state.cursor_pos);
                        }
                        state.update_suggestions();
                    }
                    KeyCode::Delete => {
                        if state.cursor_pos < state.input.len() {
                            state.input.remove(state.cursor_pos);
                        }
                        state.update_suggestions();
                    }
                    KeyCode::Left => {
                        if state.cursor_pos > 0 {
                            state.cursor_pos -= 1;
                        }
                    }
                    KeyCode::Right => {
                        if state.cursor_pos < state.input.len() {
                            state.cursor_pos += 1;
                        }
                    }
                    KeyCode::Home | KeyCode::Up => {
                        if key.code == KeyCode::Home {
                            state.cursor_pos = 0;
                        } else if state.scroll_offset > 0 {
                            state.scroll_offset -= 1;
                        }
                    }
                    KeyCode::End | KeyCode::Down => {
                        if key.code == KeyCode::End {
                            state.cursor_pos = state.input.len();
                        } else if state.scroll_offset < state.max_scroll() {
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

        while let Ok(msg) = resp_rx.try_recv() {
            if msg == "__connected__" || msg.starts_with("__error__") {
                continue;
            }
            state.entries.retain(|e| !matches!(e, ChatEntry::Typing));
            state.is_waiting = false;

            if let Ok(resp) = serde_json::from_str::<kanojod_ipc::JsonRpcResponse>(&msg) {
                if let Some(err) = resp.result["error"].as_str() {
                    state.error = Some(err.to_string());
                } else if resp.result.get("messages").is_some() {
                    // db_query response
                    state.show_debug = false;
                    state.push_entry(ChatEntry::System("=== Database State ===".into()));
                    if let Some(_e) = resp.result["emotion"].as_str() {
                        state.push_entry(ChatEntry::System(format!(
                            "Emotion: {} | Activity: {} | Anger: {} | Affection: {} | Intimacy: {}",
                            resp.result["emotion"],
                            resp.result["activity"],
                            resp.result["anger"],
                            resp.result["affection"],
                            resp.result["intimacy"],
                        )));
                    }
                    if let Some(msgs) = resp.result["messages"].as_array() {
                        for m in msgs.iter().rev().take(20) {
                            let sender = m["sender"].as_str().unwrap_or("?");
                            let dialogue = m["dialogue"].as_str().unwrap_or("?");
                            state.push_entry(ChatEntry::System(format!(
                                "[{}] {}: {}",
                                m["channel"], sender, dialogue
                            )));
                        }
                    }
                } else if resp.result.get("llm_model").is_some() {
                    // debug_info response
                    state.show_debug = true;
                    state.push_entry(ChatEntry::System(
                        "=== DEBUG INFO (dev only) ===".into(),
                    ));
                    state.push_entry(ChatEntry::System(format!(
                        "PID: {} | Socket: {}",
                        resp.result["pid"], resp.result["socket_path"],
                    )));
                    state.push_entry(ChatEntry::System(format!(
                        "Companion: {} | User: {}",
                        resp.result["companion_name"], resp.result["user_name"],
                    )));
                    state.push_entry(ChatEntry::System(format!(
                        "Emotion: {} | Activity: {} | Anger: {} | Affection: {} | Intimacy: {}",
                        resp.result["emotion"],
                        resp.result["activity"],
                        resp.result["anger"],
                        resp.result["affection"],
                        resp.result["intimacy"],
                    )));
                    state.push_entry(ChatEntry::System(format!(
                        "LLM: {} @ {} (temp={})",
                        resp.result["llm_model"],
                        resp.result["llm_base_url"],
                        resp.result["llm_temperature"],
                    )));
                    state.push_entry(ChatEntry::System(format!(
                        "DB: {}",
                        resp.result["db_path"],
                    )));
                    state.push_entry(ChatEntry::System(format!(
                        "Persona: {}",
                        resp.result["persona"],
                    )));
                } else {
                    // send_message response
                    let action = resp.result["action_text"].as_str().map(String::from);
                    let dialogue = resp.result["dialogue"]
                        .as_str()
                        .unwrap_or("...")
                        .to_string();
                    let thought = resp.result["inner_thought"].as_str().map(String::from);
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

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
