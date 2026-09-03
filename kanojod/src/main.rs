use anyhow::Result;
use kanojod_db::Database;
use kanojod_ipc::{GatewaySource, IpcEnvelope, IpcRequest};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::Mutex;

struct Daemon {
    db: Arc<Mutex<Database>>,
}

impl Daemon {
    fn new(db: Database) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
        }
    }

    async fn handle_request(&self, req: IpcRequest) -> Result<Option<String>> {
        let db = self.db.lock().await;
        match req {
            IpcRequest::SendUserMessage { source, text } => {
                let channel = match &source {
                    GatewaySource::Cli => "cli",
                    GatewaySource::Telegram { .. } => "telegram",
                    GatewaySource::DesktopNotification => "daemon",
                };
                db.insert_message("user", channel, &text, None, None)?;
                let emotion = db.process_incoming_message(&text)?;
                let reply = format!("(emotion: {}) echoing: {}", emotion, text);
                db.insert_message("companion", "daemon", &reply, None, None)?;
                Ok(Some(reply))
            }
            IpcRequest::FetchState => {
                let emotion = db.get_emotion()?;
                let activity = db.get_activity()?;
                let anger = db.get_anger()?;
                let affection = db.get_affection()?;
                Ok(Some(format!(
                    "emotion={} activity={} anger={} affection={}",
                    emotion, activity, anger, affection
                )))
            }
            IpcRequest::SetAfk { reason } => {
                db.set_activity("AFK")?;
                if let Some(r) = &reason {
                    db.insert_message("system", "daemon", r, None, None)?;
                }
                Ok(Some("afk set".into()))
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let db_path = db_path();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db = Database::open(db_path.to_str().unwrap())?;
    db.init_schema()?;

    let sock_path = socket_path();
    if let Some(parent) = sock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(&sock_path);

    let daemon = Daemon::new(Database::open(db_path.to_str().unwrap())?);

    let listener = UnixListener::bind(&sock_path)?;
    tracing::info!("kanojod listening on {}", sock_path.display());

    loop {
        let (stream, _addr) = listener.accept().await?;
        let daemon = Daemon {
            db: Arc::clone(&daemon.db),
        };

        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();

            while let Some(line) = lines.next_line().await.ok().flatten() {
                let response = match serde_json::from_str::<IpcEnvelope<IpcRequest>>(&line) {
                    Ok(envelope) => {
                        let result = daemon.handle_request(envelope.payload).await;
                        let payload = result.unwrap_or(None).unwrap_or_default();
                        IpcEnvelope::new(envelope.id, payload)
                    }
                    Err(_) => {
                        let req = IpcRequest::SendUserMessage {
                            source: GatewaySource::Cli,
                            text: line.clone(),
                        };
                        let result = daemon.handle_request(req).await;
                        let payload = result.unwrap_or(None).unwrap_or_default();
                        IpcEnvelope::new(0, payload)
                    }
                };
                let json = serde_json::to_string(&response).unwrap_or_default();
                let _ = writer.write_all(json.as_bytes()).await;
                let _ = writer.write_all(b"\n").await;
            }
        });
    }
}

fn socket_path() -> PathBuf {
    PathBuf::from("/tmp/kanojo.sock")
}

fn db_path() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .ok()
        .map(PathBuf::from)
        .map(|d| d.join("kanojo/kanojo.db"))
        .unwrap_or_else(|| {
            PathBuf::from(
                std::env::var("HOME")
                    .unwrap_or_else(|_| ".".into()),
            )
            .join(".local/share/kanojo/kanojo.db")
        })
}
