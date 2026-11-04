//! Who may fetch what from this node: the saved allow-list, contacts and
//! shares, plus in-memory copies so the checks never read the disk.

use std::{
    collections::{HashMap, HashSet},
    sync::RwLock,
};

use super::db::{Db, Share};
use crate::{
    contacts::{Contact, InviteToken, now_ms},
    error::Error,
    ids::{ContentHash, PeerId},
};

/// Shared by the node, its blob gate and its control handler.
#[derive(Debug)]
pub(super) struct Access {
    /// Peers that may connect at all.
    allowed: RwLock<HashSet<PeerId>>,
    shares: RwLock<Shares>,
    db: Db,
}

/// Our shares, indexed for the per-request check.
#[derive(Debug, Default)]
struct Shares {
    /// Share root → the share as saved (recipients, revoked, expiry).
    roots: HashMap<ContentHash, Share>,
    /// Any hash in a share (root, name list, file) → the shares containing it.
    containing: HashMap<ContentHash, HashSet<ContentHash>>,
}

impl Shares {
    /// Indexes a share as saved, replacing what was known about it.
    fn add(&mut self, root: ContentHash, share: Share) {
        for hash in &share.hashes {
            self.containing.entry(*hash).or_default().insert(root);
        }
        self.roots.insert(root, share);
    }

    /// Drops a share from the index.
    fn remove(&mut self, root: ContentHash, share: &Share) {
        self.roots.remove(&root);
        for hash in &share.hashes {
            if let Some(roots) = self.containing.get_mut(hash) {
                roots.remove(&root);
                if roots.is_empty() {
                    self.containing.remove(hash);
                }
            }
        }
    }
}

impl Access {
    pub(super) fn new(db: Db) -> Result<Self, Error> {
        let mut shares = Shares::default();
        for (root, share) in db.shares()? {
            shares.add(root, share);
        }
        Ok(Self {
            allowed: RwLock::new(db.allowed()?),
            shares: RwLock::new(shares),
            db,
        })
    }

    /// Whether `peer` may get `hash`. `as_list` means the request reads it as
    /// a list of further hashes; that is only allowed on a share's root, since
    /// a file's bytes could otherwise spell out hashes of other content.
    pub(super) fn may_get(&self, peer: PeerId, hash: ContentHash, as_list: bool) -> bool {
        let shares = self.shares.read().expect("share lock poisoned");
        let now = now_ms();
        // Only an active share counts: revoked or expired ones open nothing.
        let is_recipient = |root: &ContentHash| {
            shares
                .roots
                .get(root)
                .is_some_and(|share| share.is_active(now) && share.recipients.contains(&peer))
        };
        if as_list {
            is_recipient(&hash)
        } else {
            shares
                .containing
                .get(&hash)
                .is_some_and(|roots| roots.iter().any(is_recipient))
        }
    }

    /// Saves a share (its recipients added to any it had), then indexes it.
    /// Returns the share as saved.
    pub(super) fn add_share(&self, root: ContentHash, share: Share) -> Result<Share, Error> {
        // Lock held across the save, so two calls cannot leave memory and disk disagreeing.
        let mut shares = self.shares.write().expect("share lock poisoned");
        // Disk first: if saving fails, memory still matches what is saved.
        let saved = self.db.add_share(root, share)?;
        shares.add(root, saved.clone());
        Ok(saved)
    }

    /// Revokes a share, then updates the index. `None` if there is no such share.
    pub(super) fn revoke(&self, root: ContentHash, now: u64) -> Result<Option<Share>, Error> {
        self.change_share(root, |db| db.revoke(root, now))
    }

    /// Sets a share's expiry, then updates the index. `None` if there is no such share.
    pub(super) fn set_expiry(
        &self,
        root: ContentHash,
        expires_at: Option<u64>,
        now: u64,
    ) -> Result<Option<Share>, Error> {
        self.change_share(root, |db| db.set_expiry(root, expires_at, now))
    }

    /// Deletes a share, then drops it from the index. Returns whether there was one.
    pub(super) fn remove_share(&self, root: ContentHash) -> Result<bool, Error> {
        // Lock held across the save, so memory and disk cannot disagree.
        let mut shares = self.shares.write().expect("share lock poisoned");
        let removed = self.db.remove_share(root)?;
        if let Some(share) = &removed {
            shares.remove(root, share);
        }
        Ok(removed.is_some())
    }

    fn change_share(
        &self,
        root: ContentHash,
        save: impl FnOnce(&Db) -> Result<Option<Share>, Error>,
    ) -> Result<Option<Share>, Error> {
        // Lock held across the save, so memory and disk cannot disagree.
        let mut shares = self.shares.write().expect("share lock poisoned");
        let saved = save(&self.db)?;
        if let Some(share) = &saved {
            shares.add(root, share.clone());
        }
        Ok(saved)
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
        // Both locks held across the save (always this order), so neither copy can
        // disagree with the disk.
        let mut allowed = self.allowed.write().expect("allow-list lock poisoned");
        let mut shares = self.shares.write().expect("share lock poisoned");
        self.db.remove_contact(peer)?;
        allowed.remove(&peer);
        for share in shares.roots.values_mut() {
            share.recipients.retain(|p| *p != peer);
        }
        Ok(())
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
