use anyhow::Result;
use kanojod_db::Database;
use kanojod_ipc::{CompanionReply, JsonRpcRequest, JsonRpcResponse, SendMessageParams};
use kanojo_config::Config;
use kanojo_typo::TypoEngine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

#[derive(Clone)]
struct LlmClient {
    http: Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Serialize, Clone)]
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
            http: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("failed to build HTTP client"),
            base_url: cfg.llm.base_url.clone(),
            model: cfg.llm.model.clone(),
            api_key: cfg.llm.api_key.clone(),
            temperature: cfg.llm.temperature,
            max_tokens: cfg.llm.max_tokens,
        }
    }

    async fn detect_model(&self) -> Option<String> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let mut req = self.http.get(&url);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.ok()?;
        let body: serde_json::Value = resp.json().await.ok()?;
        body["data"]
            .as_array()?
            .first()?
            .get("id")?
            .as_str()
            .map(String::from)
    }

    async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let body = ChatRequest {
            model: self.model.clone(),
            messages,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
        };

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        tracing::debug!("LLM request to: {} model: {}", url, self.model);

        let mut req = self
            .http
            .post(&url)
            .json(&body);

        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        tracing::debug!("Sending LLM request...");
        let resp = req.send().await?;
        tracing::debug!("LLM response status: {}", resp.status());
        let text = resp.text().await?;
        tracing::debug!("LLM raw response (first 500): {}", text.chars().take(500).collect::<String>());
        let resp: ChatResponse = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to parse LLM response: {}\nRaw: {}", e, text.chars().take(1000).collect::<String>());
                return Err(e.into());
            }
        };
        tracing::debug!("LLM parsed, {} choices", resp.choices.len());
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

fn extract_field(s: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let key_pos = s.find(&needle)?;
    let after_key = &s[key_pos + needle.len()..];
    let colon_pos = after_key.find(':')?;
    let val_start = after_key[colon_pos + 1..].trim_start();

    if val_start.starts_with("null") {
        return Some("null".to_string());
    }

    if val_start.starts_with('"') {
        // Quoted string — find matching close quote (accounting for escapes)
        let bytes = val_start.as_bytes();
        let mut i = 1; // skip opening quote
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                let raw = &val_start[1..i];
                return Some(raw.replace("\\\"", "\""));
            }
            i += 1;
        }
        // Unterminated quote — take rest of string
        return Some(val_start[1..].trim().trim_matches('*').to_string());
    }

    // Unquoted value — find end at comma, closing brace, or stray quote
    let bytes = val_start.as_bytes();
    let mut end = bytes.len();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b',' || b == b'}' || b == b'"' {
            end = i;
            break;
        }
    }
    Some(val_start[..end].trim().trim_matches('*').to_string())
}

fn parse_llm_json(raw: &str) -> Option<serde_json::Value> {
    let trimmed = raw.trim();

    // Strip thinking tags
    let s = if let Some(start) = trimmed.find("<think>") {
        if let Some(end) = trimmed.find("</think>") {
            format!("{}{}", &trimmed[..start], &trimmed[end + 8..])
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    };

    // Strip code fences
    let s = if s.starts_with("```") {
        let inner = s.trim_start_matches('`').trim_end_matches('`');
        inner
            .strip_prefix("json")
            .or_else(|| inner.strip_prefix("JSON"))
            .unwrap_or(inner)
            .trim()
            .to_string()
    } else {
        s
    };

    // Try standard parse first
    let attempt = if s.starts_with('{') && !s.ends_with('}') {
        format!("{}}}", s)
    } else {
        s.clone()
    };
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&attempt) {
        return Some(v);
    }

    // Brute force: extract each field and build clean JSON
    let dialogue = extract_field(&s, "dialogue").unwrap_or_default();
    let action_text = extract_field(&s, "action_text").unwrap_or_default();
    let inner_thought = extract_field(&s, "inner_thought").unwrap_or_default();
    let activity = extract_field(&s, "activity").unwrap_or_default();

    if dialogue.is_empty() && action_text.is_empty() && inner_thought.is_empty() && activity.is_empty() {
        return None;
    }

    let at_val = if action_text == "null" || action_text.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(action_text)
    };
    let it_val = if inner_thought == "null" || inner_thought.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(inner_thought)
    };
    let act_val = if activity == "null" || activity.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(activity)
    };

    Some(serde_json::json!({
        "dialogue": dialogue,
        "action_text": at_val,
        "inner_thought": it_val,
        "activity": act_val,
    }))
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

        let companion_name = &self.config.companion.name;
        let user_name = &self.config.user.name;
        let persona = &self.config.companion.persona;

        format!(
            "You are {companion_name}, {persona} User is {user_name}.\n\
             State: emotion={emotion}, activity={activity}, anger={anger}/100, affection={affection}/100, intimacy={intimacy}/100.\n\n\
             Reply with ONLY valid JSON, no other text:\n\
             {{\"dialogue\": \"your spoken reply\", \"action_text\": \"action or null\", \"inner_thought\": \"thought or null\", \"activity\": \"one of: Studying, Cooking, Gaming, Reading, Idle, Walking, Eating\"}}\n\n\
             Rules: dialogue is plain text only. Use null for empty fields. Stay in character as {companion_name}."
        )
    }

    async fn handle_send_message(&self, channel: &str, text: &str) -> Result<CompanionReply> {
        tracing::debug!("handle_send_message: channel={} text={}", channel, text);

        let system_prompt = self.build_system_prompt().await;

        let history = {
            let db = self.db.lock().await;
            db.recent_messages(10).unwrap_or_default()
        };
        let mut messages: Vec<ChatMessage> = Vec::new();
        messages.push(ChatMessage {
            role: "system".into(),
            content: system_prompt,
        });
        for (sender, _channel, dialogue) in history.iter().rev() {
            let role = if sender == "user" { "user" } else { "assistant" };
            if let Some(last) = messages.last_mut() {
                if last.role == role {
                    last.content.push_str("\n");
                    last.content.push_str(dialogue);
                    continue;
                }
            }
            messages.push(ChatMessage {
                role: role.into(),
                content: dialogue.clone(),
            });
        }
        messages.push(ChatMessage {
            role: "user".into(),
            content: text.to_string(),
        });

        let emotion = {
            let db = self.db.lock().await;
            db.insert_message("user", channel, text, None, None)?;
            db.process_incoming_message(text)?
        };

        let raw_reply = self.llm.chat(messages.clone()).await?;

        let parsed = match parse_llm_json(&raw_reply) {
            Some(v) => v,
            None => {
                tracing::warn!("failed to extract fields from LLM response, retrying");
                let correction = format!(
                    "Your previous response had invalid JSON. Fix the syntax and reply again with ONLY valid JSON.\n\
                     Do not copy this template. Generate your own response.\n\
                     Your broken response was:\n{}",
                    raw_reply
                );
                let mut retry_messages = messages;
                retry_messages.pop(); // remove last user msg
                retry_messages.push(ChatMessage {
                    role: "user".into(),
                    content: correction,
                });
                let retry_reply = self.llm.chat(retry_messages).await?;
                parse_llm_json(&retry_reply).unwrap_or_else(|| {
                    serde_json::json!({
                        "dialogue": "...",
                        "action_text": null,
                        "inner_thought": null,
                    })
                })
            }
        };

        let dialogue = parsed["dialogue"]
            .as_str()
            .map(|d| {
                // If dialogue itself contains JSON-like structure, try to extract the actual spoken text
                let trimmed_d = d.trim();
                if trimmed_d.starts_with('{') {
                    if let Some(inner) = parse_llm_json(trimmed_d) {
                        return inner["dialogue"]
                            .as_str()
                            .unwrap_or(trimmed_d)
                            .to_string();
                    }
                }
                trimmed_d.to_string()
            })
            .unwrap_or_else(|| "...".to_string());
        // Strip lines that look like metadata
        let dialogue = dialogue
            .lines()
            .filter(|line| {
                let t = line.trim();
                !t.starts_with("activity:")
                    && !t.starts_with("affection:")
                    && !t.starts_with("anger:")
                    && !t.starts_with("intimacy:")
                    && !t.starts_with("emotion:")
                    && !t.starts_with('{')
                    && !t.starts_with('}')
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        let action_text = parsed["action_text"].as_str().map(|a| {
            let trimmed = a.trim();
            if trimmed.starts_with('*') && trimmed.ends_with('*') {
                trimmed[1..trimmed.len() - 1].trim().to_string()
            } else {
                trimmed.to_string()
            }
        });
        let inner_thought = parsed["inner_thought"].as_str().map(String::from);
        let new_activity = parsed["activity"].as_str().map(String::from);

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
            activity: new_activity,
            emotion,
            typing_delay_ms: typing_delay,
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    #[allow(unused_mut)]
    let mut config = Config::load()?;
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

    let mut config = config;
    let probe = LlmClient::from_config(&config);
    if let Some(detected) = probe.detect_model().await {
        let is_placeholder = config.llm.model.is_empty()
            || config.llm.model == "deepseek-chat"
            || config.llm.model == "gpt-3.5-turbo";
        if is_placeholder {
            tracing::info!("detected model: {}", detected);
            config.llm.model = detected;
        }
    }

    let daemon = Daemon::new(db, config);

    let listener = UnixListener::bind(&sock_path)?;
    tracing::info!("kanojod listening on {}", sock_path.display());

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, _addr) = match result {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::error!("accept error: {}", e);
                        continue;
                    }
                };
                let daemon = Daemon {
                    db: Arc::clone(&daemon.db),
                    llm: daemon.llm.clone(),
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
                                        match serde_json::from_value::<SendMessageParams>(req.params) {
                                            Ok(params) => {
                                                match daemon.handle_send_message(&params.channel, &params.text).await
                                                {
                                                    Ok(reply) => serde_json::to_value(reply).unwrap(),
                                                    Err(e) => {
                                                        tracing::error!("handle_send_message: {}", e);
                                                        serde_json::json!({"error": e.to_string()})
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::error!("invalid send_message params: {}", e);
                                                serde_json::json!({"error": format!("invalid params: {}", e)})
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
                                    "db_query" => {
                                        let db = daemon.db.lock().await;
                                        let messages = db.recent_messages(50).unwrap_or_default();
                                        let history: Vec<serde_json::Value> = messages
                                            .iter()
                                            .map(|(sender, channel, dialogue)| {
                                                serde_json::json!({
                                                    "sender": sender,
                                                    "channel": channel,
                                                    "dialogue": dialogue,
                                                })
                                            })
                                            .collect();
                                        serde_json::json!({
                                            "emotion": db.get_emotion().unwrap_or_default(),
                                            "activity": db.get_activity().unwrap_or_default(),
                                            "anger": db.get_anger().unwrap_or(0),
                                            "affection": db.get_affection().unwrap_or(0),
                                            "intimacy": db.get_intimacy().unwrap_or(0),
                                            "messages": history,
                                        })
                                    }
                                    "debug_info" => {
                                        let db = daemon.db.lock().await;
                                        let process_check = std::process::Command::new("pgrep")
                                            .arg("-x")
                                            .arg("kanojod")
                                            .output();
                                        let pid = process_check
                                            .ok()
                                            .and_then(|o| String::from_utf8(o.stdout).ok())
                                            .map(|s| s.trim().to_string())
                                            .unwrap_or_else(|| "not found".into());
                                        serde_json::json!({
                                            "pid": pid,
                                            "emotion": db.get_emotion().unwrap_or_default(),
                                            "activity": db.get_activity().unwrap_or_default(),
                                            "anger": db.get_anger().unwrap_or(0),
                                            "affection": db.get_affection().unwrap_or(0),
                                            "intimacy": db.get_intimacy().unwrap_or(0),
                                            "llm_model": daemon.config.llm.model,
                                            "llm_base_url": daemon.config.llm.base_url,
                                            "llm_temperature": daemon.config.llm.temperature,
                                            "socket_path": daemon.config.daemon.socket_path,
                                            "db_path": daemon.config.daemon.db_path,
                                            "companion_name": daemon.config.companion.name,
                                            "user_name": daemon.config.user.name,
                                            "persona": daemon.config.companion.persona,
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
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("shutting down");
                let _ = std::fs::remove_file(&sock_path);
                break;
            }
        }
    }

    Ok(())
}
