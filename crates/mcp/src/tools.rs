//! Tool registry and dispatcher.
//!
//! The MCP gateway exposes five tools that wrap `helios-store`'s
//! query API. Each tool's `inputSchema` is JSON Schema; Claude Code
//! reads it from `tools/list` and validates arguments before calling.
//!
//! All tools route through the same per-call store connection in
//! `store_client`. The dispatcher returns a `serde_json::Value` —
//! the caller wraps it in the MCP `tools/call` envelope (content
//! array of typed parts).

use crate::protocol::ToolDef;
use serde_json::{Value, json};

/// Static tool definitions.
pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "helios_ping".to_string(),
            description: "Verify the heliOS entity store is reachable. \
                          Returns the count of applied schema migrations \
                          and the running schema version."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDef {
            name: "helios_list_processes".to_string(),
            description: "List currently-running processes observed by \
                          the events bus. Most recent first."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Max rows to return (default 100, max 1000).",
                        "minimum": 1,
                        "maximum": 1000
                    }
                }
            }),
        },
        ToolDef {
            name: "helios_get_process".to_string(),
            description: "Fetch a single process by PID, including its \
                          command line, exe, uid, and start time."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pid": {"type": "integer", "description": "Process ID"}
                },
                "required": ["pid"]
            }),
        },
        ToolDef {
            name: "helios_list_recent_events".to_string(),
            description: "List the most recent events from the bus. \
                          Optionally filter by source (e.g. 'procfs', \
                          'kernel', 'dbus', 'journald')."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Max rows to return (default 100, max 1000)."
                    },
                    "source": {
                        "type": "string",
                        "description": "Filter to a single event source."
                    }
                }
            }),
        },
        ToolDef {
            name: "helios_stats".to_string(),
            description: "Aggregate dashboard counters: total processes, \
                          running processes, total events, events in the \
                          last minute, last event timestamp."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
    ]
}

/// Tool name → store request mapping. Returns an in-flight
/// `StoreRequest` ready to send over the store socket.
pub fn build_store_request(
    name: &str,
    arguments: &Value,
) -> anyhow::Result<helios_schema::ipc::StoreRequest> {
    use helios_schema::ipc::StoreRequest;
    Ok(match name {
        "helios_ping" => StoreRequest::Ping,
        "helios_list_processes" => StoreRequest::ListProcesses {
            limit: arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32),
        },
        "helios_get_process" => {
            let pid = arguments
                .get("pid")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow::anyhow!("pid required"))? as i32;
            StoreRequest::GetProcess { pid }
        }
        "helios_list_recent_events" => StoreRequest::ListRecentEvents {
            limit: arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32),
            source: arguments
                .get("source")
                .and_then(|v| v.as_str())
                .map(String::from),
        },
        "helios_stats" => StoreRequest::Stats,
        other => anyhow::bail!("unknown tool: {other}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_are_well_formed() {
        let defs = definitions();
        assert!(defs.len() >= 5);
        for d in &defs {
            assert!(!d.name.is_empty());
            assert!(!d.description.is_empty());
            assert!(d.input_schema.is_object());
        }
    }

    #[test]
    fn build_request_for_known_tool() {
        let req =
            build_store_request("helios_list_processes", &json!({"limit": 10})).unwrap();
        assert!(matches!(
            req,
            helios_schema::ipc::StoreRequest::ListProcesses { limit: Some(10) }
        ));
    }

    #[test]
    fn build_request_for_unknown_tool() {
        let r = build_store_request("nope", &json!({}));
        assert!(r.is_err());
    }

    #[test]
    fn get_process_requires_pid() {
        let r = build_store_request("helios_get_process", &json!({}));
        assert!(r.is_err());
    }
}
