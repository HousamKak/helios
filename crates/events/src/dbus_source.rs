//! D-Bus signal source.
//!
//! Connects to the system bus, subscribes to all signals, and emits
//! `EventPayload::DbusSignal` for each one. This gives the agent
//! visibility into:
//!
//! * systemd unit lifecycle and state changes (`org.freedesktop.systemd1`)
//! * NetworkManager connectivity (`org.freedesktop.NetworkManager`)
//! * UPower battery state (`org.freedesktop.UPower`)
//! * BlueZ device events (`org.bluez`)
//! * logind seat/session events (`org.freedesktop.login1`)
//!
//! ...and any other system-bus traffic without needing per-service
//! typed proxies. Phase 2 may layer typed `zbus_systemd::ManagerProxy`
//! on top to project unit state into a dedicated table; for v0.1 the
//! generic listener captures everything as events for the universal log.

use futures_util::StreamExt;
use helios_schema::{EventPayload, EventSource, SystemEvent, generate_id, now};
use tokio::sync::broadcast;
use zbus::{Connection, MatchRule, MessageStream, message::Type as MessageType};

/// Subscribe to all signals on the system bus and forward them as
/// `DbusSignal` events. Reconnects on failure.
pub async fn run(tx: broadcast::Sender<SystemEvent>) -> anyhow::Result<()> {
    use std::time::Duration;
    loop {
        match run_once(&tx).await {
            Ok(()) => {
                tracing::warn!("D-Bus stream ended cleanly; reconnecting");
            }
            Err(err) => {
                tracing::warn!(?err, "D-Bus source error; reconnecting");
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn run_once(tx: &broadcast::Sender<SystemEvent>) -> anyhow::Result<()> {
    let conn = Connection::system().await?;
    tracing::info!("connected to D-Bus system bus");

    // Match every signal on the bus. Using a single broad match rule is
    // cheaper than multiple narrow ones because the kernel filters once.
    let rule = MatchRule::builder().msg_type(MessageType::Signal).build();

    let mut stream = MessageStream::for_match_rule(rule, &conn, Some(64)).await?;

    while let Some(message_result) = stream.next().await {
        let message = match message_result {
            Ok(m) => m,
            Err(err) => {
                tracing::debug!(?err, "skipping malformed D-Bus message");
                continue;
            }
        };

        let header = message.header();
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let path = header.path().map(|p| p.to_string()).unwrap_or_default();
        let interface = header
            .interface()
            .map(|i| i.to_string())
            .unwrap_or_default();
        let member = header.member().map(|m| m.to_string()).unwrap_or_default();

        // Body parsing is type-dependent; for the generic stream we
        // emit a placeholder. Phase 2 typed proxies will populate
        // body_json with structured data per interface.
        let body_json = serde_json::Value::Null;

        let envelope = SystemEvent {
            id: generate_id(),
            timestamp: now(),
            source: EventSource::Dbus,
            correlation_id: None,
            causation_id: None,
            payload: EventPayload::DbusSignal {
                sender,
                path,
                interface,
                member,
                body_json,
            },
        };

        let _ = tx.send(envelope);
    }

    Ok(())
}
