//! JSON-RPC 2.0 + minimal MCP shape.
//!
//! MCP stdio transport uses newline-delimited JSON. Each line is one
//! complete JSON-RPC 2.0 message. Requests carry an `id` (number,
//! string, or null); notifications omit it and the server MUST NOT
//! reply to them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 request envelope.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    /// True when this is a notification (no `id`) — the server MUST
    /// NOT respond.
    pub fn is_notification(&self) -> bool {
        self.id.is_none() || matches!(self.id, Some(Value::Null))
    }
}

/// JSON-RPC 2.0 response envelope.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// Tool definition exposed via `tools/list`.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn notification_round_trip() {
        let n = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };
        assert!(n.is_notification());
    }

    #[test]
    fn response_omits_unset_fields() {
        let r = JsonRpcResponse::success(Some(json!(1)), json!({"ok": true}));
        let s = serde_json::to_string(&r).unwrap();
        assert!(!s.contains("\"error\""));
        assert!(s.contains("\"result\""));
    }

    #[test]
    fn error_response_omits_result_field() {
        let r = JsonRpcResponse::error(Some(json!(2)), -32000, "boom");
        let s = serde_json::to_string(&r).unwrap();
        assert!(!s.contains("\"result\""));
        assert!(s.contains("\"error\""));
        assert!(s.contains("boom"));
    }
}
