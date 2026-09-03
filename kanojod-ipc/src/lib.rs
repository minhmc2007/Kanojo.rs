use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub result: serde_json::Value,
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub jsonrpc: String,
    pub error: JsonRpcErrorBody,
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcErrorBody {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageParams {
    pub channel: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionReply {
    pub action_text: Option<String>,
    pub dialogue: String,
    pub inner_thought: Option<String>,
    pub emotion: String,
    pub typing_delay_ms: u64,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: &str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
            id,
        }
    }

    pub fn send_message(id: u64, channel: &str, text: &str) -> Self {
        let params = serde_json::to_value(SendMessageParams {
            channel: channel.into(),
            text: text.into(),
        })
        .unwrap_or(serde_json::Value::Null);
        Self::new(id, "send_message", params)
    }

    pub fn fetch_state(id: u64) -> Self {
        Self::new(id, "fetch_state", serde_json::Value::Null)
    }
}

impl JsonRpcResponse {
    pub fn new(id: u64, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            result,
            id,
        }
    }
}
