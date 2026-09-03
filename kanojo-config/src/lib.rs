use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub user: UserConfig,
    pub companion: CompanionConfig,
    pub llm: LlmConfig,
    pub daemon: DaemonConfig,
    pub telegram: Option<TelegramConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionConfig {
    pub name: String,
    pub persona: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub temperature: f32,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub socket_path: String,
    pub db_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub allowed_chat_ids: Vec<i64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            user: UserConfig {
                name: "User".into(),
            },
            companion: CompanionConfig {
                name: "Aoi".into(),
                persona: "A warm, casual companion who enjoys conversation.".into(),
            },
            llm: LlmConfig {
                provider: "openai-compatible".into(),
                model: "deepseek-chat".into(),
                base_url: "http://127.0.0.1:8080/v1".into(),
                api_key: None,
                temperature: 0.8,
                max_tokens: 512,
            },
            daemon: DaemonConfig {
                socket_path: default_socket_path()
                    .to_string_lossy()
                    .into_owned(),
                db_path: default_db_path()
                    .to_string_lossy()
                    .into_owned(),
            },
            telegram: None,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            toml::from_str(&content).with_context(|| "parsing config.toml")
        } else {
            let cfg = Self::default();
            cfg.save()?;
            Ok(cfg)
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kanojo/config.toml")
}

fn default_socket_path() -> PathBuf {
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(runtime).join("kanojo.sock")
}

fn default_db_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kanojo/kanojo.db")
}
