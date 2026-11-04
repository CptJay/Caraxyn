//! The JSON the UI sees. Mirrored by hand in `ui/src/lib/api.ts`: change both together.
//!
//! Ids travel as 64-character hex strings (their `Display`); the core's own
//! serde form is raw bytes, which suits postcard but not JavaScript.

use caraxyn_core::{
    contacts::Contact,
    events::Event,
    shares::{MyShare, Offer, OfferState, PendingOffer},
};
use serde::Serialize;

/// A contact.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactDto {
    pub peer: String,
    pub name: String,
    pub added_at: u64,
}

impl From<Contact> for ContactDto {
    fn from(c: Contact) -> Self {
        Self {
            peer: c.peer.to_string(),
            name: c.name,
            added_at: c.added_at,
        }
    }
}

/// An offer in the inbox.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferDto {
    pub from: String,
    pub root: String,
    pub title: String,
    pub file_count: u64,
    pub total_size: u64,
    pub preview: Vec<(String, u64)>,
    pub expires_at: Option<u64>,
    pub received_at: u64,
    /// `new`, `declined`, `done` or `revoked`.
    pub state: &'static str,
}

impl From<Offer> for OfferDto {
    fn from(o: Offer) -> Self {
        Self {
            from: o.from.to_string(),
            root: o.root.to_string(),
            title: o.title,
            file_count: o.file_count,
            total_size: o.total_size,
            preview: o.preview,
            expires_at: o.expires_at,
            received_at: o.received_at,
            state: match o.state {
                OfferState::New => "new",
                OfferState::Declined => "declined",
                OfferState::Done => "done",
                OfferState::Revoked => "revoked",
            },
        }
    }
}

/// One of our shares.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareDto {
    pub root: String,
    pub title: String,
    pub recipients: Vec<String>,
    pub file_count: u64,
    pub total_size: u64,
    pub revoked_at: Option<u64>,
    pub expires_at: Option<u64>,
}

impl From<MyShare> for ShareDto {
    fn from(s: MyShare) -> Self {
        Self {
            root: s.root.to_string(),
            title: s.title,
            recipients: s.recipients.iter().map(ToString::to_string).collect(),
            file_count: s.file_count,
            total_size: s.total_size,
            revoked_at: s.revoked_at,
            expires_at: s.expires_at,
        }
    }
}

/// An offer or revoke still on its way to a recipient.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingDto {
    pub to: String,
    pub root: String,
    pub revoke: bool,
    pub attempts: u32,
}

impl From<PendingOffer> for PendingDto {
    fn from(p: PendingOffer) -> Self {
        Self {
            to: p.to.to_string(),
            root: p.root.to_string(),
            revoke: p.revoke,
            attempts: p.attempts,
        }
    }
}

/// A node event, sent to the UI as `node-event`.
#[derive(Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NodeEvent {
    ContactAdded {
        contact: ContactDto,
    },
    OfferReceived {
        offer: OfferDto,
    },
    OfferRevoked {
        from: String,
        root: String,
    },
    Delivered {
        to: String,
        root: String,
        revoke: bool,
    },
    Undeliverable {
        to: String,
        root: String,
        revoke: bool,
    },
    Progress {
        from: String,
        root: String,
        bytes: u64,
        total: Option<u64>,
    },
    Warning {
        context: &'static str,
        message: String,
    },
}

impl NodeEvent {
    /// The UI form of `event`, or `None` for a kind this app does not know yet.
    pub fn from_core(event: Event) -> Option<Self> {
        Some(match event {
            Event::ContactAdded { contact } => Self::ContactAdded {
                contact: contact.into(),
            },
            Event::OfferReceived { offer } => Self::OfferReceived {
                offer: offer.into(),
            },
            Event::OfferRevoked { from, root } => Self::OfferRevoked {
                from: from.to_string(),
                root: root.to_string(),
            },
            Event::Delivered { to, root, revoke } => Self::Delivered {
                to: to.to_string(),
                root: root.to_string(),
                revoke,
            },
            Event::Undeliverable { to, root, revoke } => Self::Undeliverable {
                to: to.to_string(),
                root: root.to_string(),
                revoke,
            },
            Event::Progress {
                from,
                root,
                bytes,
                total,
            } => Self::Progress {
                from: from.to_string(),
                root: root.to_string(),
                bytes,
                total,
            },
            Event::Warning { context, message } => Self::Warning { context, message },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use caraxyn_core::ids::{ContentHash, PeerId};
    use serde_json::json;

    use super::*;

    // The TypeScript side switches on these exact names.
    #[test]
    fn events_serialize_as_the_ui_expects() {
        let event = Event::Progress {
            from: PeerId::from([1; 32]),
            root: ContentHash::from([2; 32]),
            bytes: 5,
            total: None,
        };
        let value = serde_json::to_value(NodeEvent::from_core(event).unwrap()).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "progress",
                "from": "01".repeat(32),
                "root": "02".repeat(32),
                "bytes": 5,
                "total": null,
            })
        );
    }

    #[test]
    fn offers_use_camel_case_and_lowercase_states() {
        let offer = Offer {
            from: PeerId::from([1; 32]),
            root: ContentHash::from([2; 32]),
            title: "t".into(),
            file_count: 1,
            total_size: 2,
            preview: vec![("t".into(), 2)],
            expires_at: None,
            received_at: 3,
            state: OfferState::Revoked,
        };
        let value = serde_json::to_value(OfferDto::from(offer)).unwrap();
        assert_eq!(value["fileCount"], 1);
        assert_eq!(value["receivedAt"], 3);
        assert_eq!(value["state"], "revoked");
    }
}
