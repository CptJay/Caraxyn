//! Forwards the node's events to the UI.

use caraxyn_core::node::Node;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;

use crate::dto::NodeEvent;

/// Emits every node event as `node-event`. After falling behind it emits
/// `resync` instead, and the UI reloads its lists.
pub fn forward(app: AppHandle, node: &Node) {
    let mut events = node.events();
    tauri::async_runtime::spawn(async move {
        loop {
            let sent = match events.recv().await {
                Ok(event) => match NodeEvent::from_core(event) {
                    Some(event) => app.emit("node-event", event),
                    None => continue,
                },
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "UI fell behind on node events");
                    app.emit("resync", ())
                }
                Err(RecvError::Closed) => break,
            };
            if let Err(e) = sent {
                tracing::warn!(%e, "could not send an event to the UI");
            }
        }
    });
}
