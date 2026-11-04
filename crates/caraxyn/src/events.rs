//! Forwards the node's events to the UI.

use caraxyn_core::{
    events::Event,
    node::Node,
    shares::{Offer, OfferState},
};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;

use crate::{desktop, dto::NodeEvent};

/// Emits every node event as `node-event`. After falling behind it emits
/// `resync` instead, and the UI reloads its lists. A new offer also gets a
/// desktop notification.
pub fn forward(app: AppHandle, node: &Node) {
    let mut events = node.events();
    let node = node.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let sent = match events.recv().await {
                Ok(event) => match NodeEvent::from_core(event.clone()) {
                    Some(ui_event) => {
                        if let Event::OfferReceived { offer } = &event {
                            notify_offer(&app, &node, offer);
                        }
                        app.emit("node-event", ui_event)
                    }
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

/// "Sam shared holiday-photos (700.0 MB)", for an offer still waiting for an answer.
fn notify_offer(app: &AppHandle, node: &Node, offer: &Offer) {
    if offer.state != OfferState::New {
        return;
    }
    let sender = node
        .contacts()
        .ok()
        .and_then(|contacts| contacts.into_iter().find(|c| c.peer == offer.from))
        .map_or_else(|| offer.from.short(), |contact| contact.name);
    let body = format!(
        "{sender} shared {} ({})",
        offer.title,
        desktop::size(offer.total_size)
    );
    desktop::notify(app, "New share", &body);
}
