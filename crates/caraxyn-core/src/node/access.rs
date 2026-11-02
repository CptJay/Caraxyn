//! Who may fetch from this node: the saved allow-list and contacts, plus an
//! in-memory copy of the allow-list so the connection check never reads the disk.

use std::{collections::HashSet, sync::RwLock};

use super::db::Db;
use crate::{
    contacts::{Contact, InviteToken},
    error::Error,
    ids::PeerId,
};

/// Shared by the node and its control handler.
#[derive(Debug)]
pub(super) struct Access {
    allowed: RwLock<HashSet<PeerId>>,
    db: Db,
}

impl Access {
    pub(super) fn new(db: Db) -> Result<Self, Error> {
        Ok(Self {
            allowed: RwLock::new(db.allowed()?),
            db,
        })
    }

    pub(super) fn db(&self) -> &Db {
        &self.db
    }

    /// Whether `peer` may fetch from us. Memory only.
    pub(super) fn is_allowed(&self, peer: PeerId) -> bool {
        self.allowed
            .read()
            .expect("allow-list lock poisoned")
            .contains(&peer)
    }

    pub(super) fn set_allowed(&self, peer: PeerId, allowed: bool) -> Result<(), Error> {
        self.change(peer, allowed, |db| {
            db.set_allowed(peer, allowed).map(|()| true)
        })
        .map(drop)
    }

    pub(super) fn add_contact(&self, contact: &Contact) -> Result<(), Error> {
        self.change(contact.peer, true, |db| {
            db.add_contact(contact).map(|()| true)
        })
        .map(drop)
    }

    pub(super) fn remove_contact(&self, peer: PeerId) -> Result<(), Error> {
        self.change(peer, false, |db| db.remove_contact(peer).map(|()| true))
            .map(drop)
    }

    /// Adds `contact` if `token` is valid at `now`; returns whether it was.
    pub(super) fn redeem(
        &self,
        token: InviteToken,
        contact: &Contact,
        now: u64,
    ) -> Result<bool, Error> {
        self.change(contact.peer, true, |db| db.redeem(token, contact, now))
    }

    /// Runs `save` on the database, then, if it changed something, updates
    /// the in-memory list to match.
    fn change(
        &self,
        peer: PeerId,
        allowed: bool,
        save: impl FnOnce(&Db) -> Result<bool, Error>,
    ) -> Result<bool, Error> {
        // Lock held across the save, so two changes cannot leave memory and disk disagreeing.
        let mut list = self.allowed.write().expect("allow-list lock poisoned");
        // Disk first: if saving fails, memory still matches what is saved.
        let changed = save(&self.db)?;
        if changed {
            if allowed {
                list.insert(peer);
            } else {
                list.remove(&peer);
            }
        }
        Ok(changed)
    }
}
