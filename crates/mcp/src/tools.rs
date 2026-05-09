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
        ToolDef {
            name: "helios_list_canvas_entities".to_string(),
            description: "List entities currently placed on the heliOS \
                          canvas. Each row carries an `id` (the canvas \
                          entity id, used as the move target), \
                          `entity_kind`, world coordinates (x, y), and \
                          a desktop scope. Optionally filter by kind \
                          (e.g. 'process', 'applet', 'agent') or by \
                          desktop_id."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "description": "Filter to a single entity kind (process, file, applet, agent, terminal, task, project, connection, desktop)."
                    },
                    "desktop_id": {
                        "type": "string",
                        "description": "Filter to a single desktop's entities."
                    }
                }
            }),
        },
        ToolDef {
            name: "helios_move_entity".to_string(),
            description: "Move a canvas entity (window) to absolute world \
                          coordinates (x, y). Pass an `id` from \
                          `helios_list_canvas_entities`. The compositor \
                          observes the resulting EntityPlaced event on \
                          the bus and visibly repositions the surface."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Canvas entity id (from helios_list_canvas_entities)."},
                    "x": {"type": "number", "description": "World-space x in pixels."},
                    "y": {"type": "number", "description": "World-space y in pixels."}
                },
                "required": ["id", "x", "y"]
            }),
        },
    ]
}

/// Parse a snake_case entity-kind string from a tool argument.
/// EntityKind in helios_schema serializes via serde rename_all =
/// snake_case but doesn't expose a FromStr. Round-tripping through
/// serde_json keeps the behaviour identical to JSON deserialization.
fn parse_entity_kind(s: &str) -> anyhow::Result<helios_schema::EntityKind> {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .map_err(|e| anyhow::anyhow!("invalid kind '{s}': {e}"))
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
        "helios_list_canvas_entities" => {
            let kind = match arguments.get("kind").and_then(|v| v.as_str()) {
                None => None,
                Some(s) => Some(parse_entity_kind(s)?),
            };
            let desktop_id = arguments
                .get("desktop_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            StoreRequest::ListCanvasEntities { kind, desktop_id }
        }
        "helios_move_entity" => {
            let id = arguments
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("id required"))?
                .to_string();
            let x = arguments
                .get("x")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow::anyhow!("x required"))?;
            let y = arguments
                .get("y")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow::anyhow!("y required"))?;
            StoreRequest::MoveEntity { id, x, y }
        }
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
        let req = build_store_request("helios_list_processes", &json!({"limit": 10})).unwrap();
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

    #[test]
    fn build_request_for_move_entity() {
        let req = build_store_request(
            "helios_move_entity",
            &json!({"id": "01HABC", "x": 300.0, "y": 400.0}),
        )
        .unwrap();
        match req {
            helios_schema::ipc::StoreRequest::MoveEntity { id, x, y } => {
                assert_eq!(id, "01HABC");
                assert_eq!(x, 300.0);
                assert_eq!(y, 400.0);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn move_entity_requires_all_three() {
        assert!(build_store_request("helios_move_entity", &json!({"x": 1.0, "y": 2.0})).is_err());
        assert!(build_store_request("helios_move_entity", &json!({"id": "x", "y": 2.0})).is_err());
        assert!(build_store_request("helios_move_entity", &json!({"id": "x", "x": 1.0})).is_err());
    }
}
