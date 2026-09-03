use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GatewaySource {
    Cli,
    Telegram { chat_id: i64 },
    DesktopNotification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcRequest {
    SendUserMessage {
        source: GatewaySource,
        text: String,
    },
    FetchState,
    SetAfk {
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcEvent {
    CompanionTyping {
        estimated_delay_ms: u64,
    },
    CompanionReply {
        action_text: Option<String>,
        dialogue: String,
        source: GatewaySource,
    },
    StatusChanged {
        emotion: String,
        activity: String,
    },
    BusyWaiting {
        queue_position: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcEnvelope<T> {
    pub id: u64,
    pub payload: T,
}

impl<T> IpcEnvelope<T> {
    pub fn new(id: u64, payload: T) -> Self {
        Self { id, payload }
    }
}
