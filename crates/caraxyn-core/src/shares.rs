//! What a sender tells a receiver about a share, and what the receiver keeps.

use serde::{Deserialize, Serialize};

use crate::ids::{ContentHash, PeerId};

/// How many files an offer lists by name; the full list comes with the fetch.
/// Keeps an offer for a huge folder small.
pub const PREVIEW_LEN: usize = 200;

/// An offer still waiting to reach its recipient (offline, or not reached yet).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingOffer {
    /// Who it is for.
    pub to: PeerId,
    /// The share's hash.
    pub root: ContentHash,
    /// How many times delivery failed so far.
    pub attempts: u32,
}

/// A share another device offered us, as kept in the inbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    /// The device that offered it.
    pub from: PeerId,
    /// The share's hash.
    pub root: ContentHash,
    /// The shared file's or folder's name.
    pub title: String,
    /// How many files the share holds.
    pub file_count: u64,
    /// Their total size in bytes, as the sender reported it.
    pub total_size: u64,
    /// The first files (at most [`PREVIEW_LEN`]): path in the share and size.
    pub preview: Vec<(String, u64)>,
    /// When it arrived, in unix milliseconds.
    pub received_at: u64,
    /// What we did with it.
    pub state: OfferState,
}

/// What the receiver did with an offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfferState {
    /// Not answered yet.
    New,
    /// Declined; an offer of the same share again does not undo this.
    Declined,
    /// Accepted, fetched and saved.
    Done,
}
