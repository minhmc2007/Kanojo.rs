use anyhow::Result;
use kanojod_db::Database;
use kanojod_ipc::{CompanionReply, JsonRpcRequest, JsonRpcResponse, SendMessageParams};
use kanojo_config::Config;
use kanojo_typo::TypoEngine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

struct LlmClient {
    http: Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMsg,
}

#[derive(Deserialize)]
struct ChatResponseMsg {
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

impl LlmClient {
    fn from_config(cfg: &Config) -> Self {
        Self {
            http: Client::new(),
            base_url: cfg.llm.base_url.clone(),
            model: cfg.llm.model.clone(),
            api_key: cfg.llm.api_key.clone(),
            temperature: cfg.llm.temperature,
            max_tokens: cfg.llm.max_tokens,
        }
    }

    async fn chat(&self, system: &str, user_msg: &str) -> Result<String> {
        let body = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: system.into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: user_msg.into(),
                },
            ],
            temperature: self.temperature,
            max_tokens: self.max_tokens,
        };

        let mut req = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(&body);

        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let resp = req.send().await?.json::<ChatResponse>().await?;
        Ok(resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default())
    }
}

struct Daemon {
    db: Arc<Mutex<Database>>,
    llm: LlmClient,
    config: Config,
}

impl Daemon {
    fn new(db: Database, config: Config) -> Self {
        let llm = LlmClient::from_config(&config);
        Self {
            db: Arc::new(Mutex::new(db)),
            llm,
            config,
        }
    }

    async fn build_system_prompt(&self) -> String {
        let db = self.db.lock().await;
        let emotion = db.get_emotion().unwrap_or_else(|_| "Casual".into());
        let activity = db.get_activity().unwrap_or_else(|_| "Idle".into());
        let anger = db.get_anger().unwrap_or(0);
        let affection = db.get_affection().unwrap_or(50);
        let intimacy = db.get_intimacy().unwrap_or(0);

        let recent = db.recent_messages(10).unwrap_or_default();
        let history: String = recent
            .iter()
            .rev()
            .map(|(sender, _channel, dialogue)| format!("{}: {}", sender, dialogue))
            .collect::<Vec<_>>()
            .join("\n");

        let companion_name = &self.config.companion.name;
        let user_name = &self.config.user.name;
        let persona = &self.config.companion.persona;

        format!(
            "You are {companion_name}, a virtual companion. {persona}\n\
             User is {user_name}.\n\n\
             CURRENT STATE:\n\
             - Emotion: {emotion}\n\
             - Activity: {activity}\n\
             - Anger: {anger}/100\n\
             - Affection: {affection}/100\n\
             - Intimacy: {intimacy}/100\n\n\
             RULES:\n\
             - Stay in character as {companion_name}.\n\
             - Your emotion affects how you respond. If angry, be short/cold. If affectionate, be warm.\n\
             - Keep replies short and natural, like a real text conversation.\n\
             - You can include action text like *shifts in seat* or *looks up from phone*\n\
             - Respond with JSON: {{\"action_text\": \"...\", \"dialogue\": \"...\", \"inner_thought\": \"...\", \"activity\": \"...\"}}\n\
             - action_text and inner_thought can be null.\n\
             - activity is what you are currently doing (e.g. \"Studying\", \"Cooking\", \"Gaming\", \"Idle\").\n\
             - Only change activity if it genuinely shifts based on the conversation.\n\n\
             RECENT CONVERSATION:\n{history}"
        )
    }

    async fn handle_send_message(&self, channel: &str, text: &str) -> Result<CompanionReply> {
        let emotion = {
            let db = self.db.lock().await;
            db.insert_message("user", channel, text, None, None)?;
            db.process_incoming_message(text)?
        };

        let system_prompt = self.build_system_prompt().await;
        let raw_reply = self.llm.chat(&system_prompt, text).await?;

        let parsed: serde_json::Value =
            serde_json::from_str(&raw_reply).unwrap_or(serde_json::json!({
                "dialogue": raw_reply.trim().trim_matches('"'),
                "action_text": null,
                "inner_thought": null,
            }));

        let dialogue = parsed["dialogue"]
            .as_str()
            .unwrap_or("...")
            .to_string();
        let action_text = parsed["action_text"].as_str().map(String::from);
        let inner_thought = parsed["inner_thought"].as_str().map(String::from);
        let new_activity = parsed["activity"].as_str().map(String::from);

        // Update activity if LLM reported a change
        if let Some(ref act) = new_activity {
            if !act.is_empty() {
                let db = self.db.lock().await;
                db.set_activity(act)?;
            }
        }

        let activity = {
            let db = self.db.lock().await;
            db.get_activity().unwrap_or_else(|_| "Idle".into())
        };
        let typo_rate = TypoEngine::rate_for_activity(&activity);
        let final_dialogue = TypoEngine::inject_typos(&dialogue, typo_rate);

        let typing_delay = 950 + (dialogue.len() as u64 * 30);

        {
            let db = self.db.lock().await;
            db.insert_message(
                "companion",
                channel,
                &final_dialogue,
                action_text.as_deref(),
                inner_thought.as_deref(),
            )?;
            db.update_last_bot_message()?;
        }

        Ok(CompanionReply {
            action_text,
            dialogue: final_dialogue,
            inner_thought,
            emotion,
            typing_delay_ms: typing_delay,
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = Config::load()?;
    let db_path = &config.daemon.db_path;
    if let Some(parent) = std::path::Path::new(db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = Database::open(db_path)?;
    db.init_schema()?;

    let sock_path = std::path::PathBuf::from(&config.daemon.socket_path);
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(&sock_path);

    let daemon = Daemon::new(Database::open(db_path)?, config);

    let listener = UnixListener::bind(&sock_path)?;
    tracing::info!("kanojod listening on {}", sock_path.display());

    loop {
        let (stream, _addr) = listener.accept().await?;
        let daemon = Daemon {
            db: Arc::clone(&daemon.db),
            llm: LlmClient {
                http: Client::new(),
                base_url: daemon.llm.base_url.clone(),
                model: daemon.llm.model.clone(),
                api_key: daemon.llm.api_key.clone(),
                temperature: daemon.llm.temperature,
                max_tokens: daemon.llm.max_tokens,
            },
            config: daemon.config.clone(),
        };

        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();

            while let Some(line) = lines.next_line().await.ok().flatten() {
                let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
                    Ok(req) => {
                        let result = match req.method.as_str() {
                            "send_message" => {
                                let params: SendMessageParams =
                                    serde_json::from_value(req.params).unwrap();
                                match daemon.handle_send_message(&params.channel, &params.text).await
                                {
                                    Ok(reply) => serde_json::to_value(reply).unwrap(),
                                    Err(e) => {
                                        tracing::error!("handle_send_message: {}", e);
                                        serde_json::json!({"error": e.to_string()})
                                    }
                                }
                            }
                            "fetch_state" => {
                                let db = daemon.db.lock().await;
                                serde_json::json!({
                                    "emotion": db.get_emotion().unwrap_or_default(),
                                    "activity": db.get_activity().unwrap_or_default(),
                                    "anger": db.get_anger().unwrap_or(0),
                                    "affection": db.get_affection().unwrap_or(0),
                                })
                            }
                            _ => serde_json::json!({"error": "unknown method"}),
                        };
                        JsonRpcResponse::new(req.id, result)
                    }
                    Err(_) => {
                        JsonRpcResponse::new(0, serde_json::json!({"error": "invalid jsonrpc"}))
                    }
                };
                let json = serde_json::to_string(&response).unwrap_or_default();
                let _ = writer.write_all(json.as_bytes()).await;
                let _ = writer.write_all(b"\n").await;
            }
        });
    }
}
