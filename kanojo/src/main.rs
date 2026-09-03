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
                println!("kanojod is running (pid: {})", String::from_utf8_lossy(&output.stdout).trim());
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

fn cmd_tui() -> Result<()> {
    use kanojod_ipc::{JsonRpcRequest, JsonRpcResponse};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let config = Config::load()?;
    let socket_path = &config.daemon.socket_path;

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let stream = UnixStream::connect(socket_path).await?;
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        println!("Connected to kanojod. Type your messages (Ctrl+D to quit).\n");

        let mut msg_id: u64 = 1;
        let stdin = io::stdin();
        loop {
            print!("{}> ", config.user.name);
            io::stdout().flush()?;
            let mut input = String::new();
            match stdin.read_line(&mut input) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            let text = input.trim();
            if text.is_empty() {
                continue;
            }

            let req = JsonRpcRequest::send_message(msg_id, "terminal", text);
            let json = serde_json::to_string(&req)?;
            writer.write_all(json.as_bytes()).await?;
            writer.write_all(b"\n").await?;

            if let Some(line) = lines.next_line().await? {
                if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&line) {
                    if let Some(action) = resp.result["action_text"].as_str() {
                        println!("  *{}*", action);
                    }
                    if let Some(dialogue) = resp.result["dialogue"].as_str() {
                        println!("  {}: {}", config.companion.name, dialogue);
                    }
                    if let Some(thought) = resp.result["inner_thought"].as_str() {
                        println!("  (thought: {})", thought);
                    }
                    if let Some(emotion) = resp.result["emotion"].as_str() {
                        println!("  [{}]", emotion);
                    }
                }
            }
            msg_id += 1;
        }
        Ok(())
    })
}
