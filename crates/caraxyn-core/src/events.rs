//! What a running node tells its listeners (a GUI) as things happen.
//!
//! Get a listener with [`Node::events`](crate::node::Node::events). Every
//! listener receives every event. One that falls far behind loses the oldest
//! ones and is told so (`Lagged`); it can catch up by reading the current
//! state again (inbox, contacts, pending offers).

use crate::{
    contacts::Contact,
    ids::{ContentHash, PeerId},
    shares::Offer,
};

/// How many events a slow listener may fall behind before it loses the oldest.
pub(crate) const EVENT_BUFFER: usize = 256;

/// Something that happened in the node.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A contact was added, on either side of an invite.
    ContactAdded {
        /// The new contact.
        contact: Contact,
    },
    /// An offer arrived in the inbox, or arrived again with new details.
    OfferReceived {
        /// The offer as it now stands in the inbox.
        offer: Offer,
    },
    /// The sender of an offer in the inbox revoked it.
    OfferRevoked {
        /// Who revoked it.
        from: PeerId,
        /// The share.
        root: ContentHash,
    },
    /// One of our offers (or revokes) reached its recipient.
    Delivered {
        /// The recipient.
        to: PeerId,
        /// The share.
        root: ContentHash,
        /// `true` for the news of a revoke, `false` for the offer.
        revoke: bool,
    },
    /// One of our offers (or revokes) was refused; it will not be retried.
    Undeliverable {
        /// The recipient.
        to: PeerId,
        /// The share.
        root: ContentHash,
        /// `true` for the news of a revoke, `false` for the offer.
        revoke: bool,
    },
    /// A download is under way (at most a few times per second, and once at the end).
    Progress {
        /// Who it comes from.
        from: PeerId,
        /// The share.
        root: ContentHash,
        /// Bytes we have so far.
        bytes: u64,
        /// The expected total, when an offer told us.
        total: Option<u64>,
    },
    /// Something went wrong in background work no caller is waiting on.
    Warning {
        /// What the node was doing.
        context: &'static str,
        /// What went wrong.
        message: String,
    },
}

/// Sends `event` to every listener; with no listener it is simply dropped.
pub(crate) fn tell(events: &tokio::sync::broadcast::Sender<Event>, event: Event) {
    // An error only means nobody is listening right now.
    events.send(event).ok();
}

/// Logs a background problem and tells listeners about it.
pub(crate) fn warn_about(
    events: &tokio::sync::broadcast::Sender<Event>,
    context: &'static str,
    error: impl std::fmt::Display,
) {
    tracing::warn!(%error, "{context}");
    tell(
        events,
        Event::Warning {
            context,
            message: error.to_string(),
        },
    );
}
