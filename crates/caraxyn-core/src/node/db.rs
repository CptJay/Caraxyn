//! Caraxyn's own small database, kept in one file next to the blob store.

use std::{collections::HashSet, path::Path};

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition, WriteTransaction};

use crate::{
    contacts::{Contact, InviteToken, check_invite},
    error::Error,
    ids::{ContentHash, PeerId},
    shares::{Offer, OfferState},
};

use serde::{Deserialize, Serialize};

/// The layout of the tables below. Bump it whenever a table or a stored type
/// changes, so an older or newer data folder is refused with a clear error.
pub(super) const FORMAT: u32 = 1;

/// About the data folder itself. Key `"format"`: its [`FORMAT`].
const META: TableDefinition<&str, u32> = TableDefinition::new("meta");

/// Peers allowed to fetch from us. Key: the peer's id; no value yet.
const ALLOWED: TableDefinition<&[u8; 32], ()> = TableDefinition::new("allowed_peers");

/// Invites we issued. Key: the token; value: when it was issued (unix ms) and whether it was used.
const INVITES: TableDefinition<&[u8; 16], (u64, bool)> = TableDefinition::new("invites");

/// Our contacts. Key: the peer's id; value: the [`Contact`] in postcard.
const CONTACTS: TableDefinition<&[u8; 32], &[u8]> = TableDefinition::new("contacts");

/// Our shares. Key: the share's root hash; value: the [`Share`] in postcard.
const SHARES: TableDefinition<&[u8; 32], &[u8]> = TableDefinition::new("shares");

/// Offers others sent us. Key: sender id then share root; value: the [`Offer`] in postcard.
const INBOX: TableDefinition<&[u8; 64], &[u8]> = TableDefinition::new("inbox");

/// Messages we still have to deliver. Key: recipient id then share root;
/// value: what it is ([`Mail`] as a byte), failed attempts so far and when to
/// try next (unix ms; 0 = now).
const OUTBOX: TableDefinition<&[u8; 64], (u8, u32, u64)> = TableDefinition::new("outbox");

/// What waits in the outbox for one recipient and share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mail {
    /// The share's offer.
    Offer = 0,
    /// The news that it was revoked.
    Revoke = 1,
}

impl Mail {
    fn from_byte(byte: u8) -> Self {
        if byte == Mail::Revoke as u8 {
            Mail::Revoke
        } else {
            Mail::Offer
        }
    }
}

/// One message waiting in the outbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OutboxEntry {
    pub(super) to: PeerId,
    pub(super) root: ContentHash,
    pub(super) mail: Mail,
    pub(super) attempts: u32,
    /// When to try next, in unix ms.
    pub(super) next_try: u64,
}

/// Who a share is for, every hash it is made of, and what its offer says.
// shortcut: postcard rows have no format version, so adding a field makes older rows unreadable; add a version before the first release.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Share {
    /// Peers allowed to fetch it.
    pub(super) recipients: Vec<PeerId>,
    /// The root, its list of names and every file: all a recipient may ask for.
    pub(super) hashes: Vec<ContentHash>,
    /// The shared file's or folder's name.
    pub(super) title: String,
    /// How many files it holds.
    pub(super) file_count: u64,
    /// Their total size in bytes.
    pub(super) total_size: u64,
    /// The first files by path and size, as sent in offers.
    pub(super) preview: Vec<(String, u64)>,
    /// When it was revoked (unix ms), if it was.
    pub(super) revoked_at: Option<u64>,
    /// When it stops being served (unix ms), if ever.
    pub(super) expires_at: Option<u64>,
}

impl Share {
    /// Whether it is still served at time `now`: not revoked, not expired.
    pub(super) fn is_active(&self, now: u64) -> bool {
        self.revoked_at.is_none() && self.expires_at.is_none_or(|at| now < at)
    }
}

/// Inbox and outbox key: the other peer's id followed by the share root.
fn pair_key(from: PeerId, root: ContentHash) -> [u8; 64] {
    let mut key = [0; 64];
    key[..32].copy_from_slice(from.as_ref());
    key[32..].copy_from_slice(root.as_ref());
    key
}

/// Handle to the database file.
#[derive(Debug)]
pub(super) struct Db(Database);

// shortcut: writes run on the caller's thread (a few ms each, fine for an allow-list); move them off the async runtime when tables get busy.
impl Db {
    /// Opens the database at `path`, creating the file and its tables if needed.
    /// A database in another [`FORMAT`] is refused, untouched.
    // shortcut: no migrations; a different format is refused. Add them with the first release that changes a format.
    pub(super) fn open(path: &Path) -> Result<Self, Error> {
        let db = Database::create(path).map_err(Error::database)?;
        let tx = db.begin_write().map_err(Error::database)?;
        // Checked before any other table is opened: an older layout would fail
        // there with an unclear error.
        let found = {
            let fresh = tx.list_tables().map_err(Error::database)?.next().is_none();
            let mut meta = tx.open_table(META).map_err(Error::database)?;
            let stored = meta
                .get("format")
                .map_err(Error::database)?
                .map(|row| row.value());
            match stored {
                Some(format) => format,
                None if fresh => {
                    meta.insert("format", FORMAT).map_err(Error::database)?;
                    FORMAT
                }
                // Tables but no number: written before formats were numbered.
                None => 0,
            }
        };
        if found != FORMAT {
            // Dropping the transaction leaves the file exactly as it was.
            return Err(Error::IncompatibleData {
                found,
                expected: FORMAT,
            });
        }
        // Creates the tables on first open, so reads never find them missing.
        tx.open_table(ALLOWED).map_err(Error::database)?;
        tx.open_table(INVITES).map_err(Error::database)?;
        tx.open_table(CONTACTS).map_err(Error::database)?;
        tx.open_table(SHARES).map_err(Error::database)?;
        tx.open_table(INBOX).map_err(Error::database)?;
        tx.open_table(OUTBOX).map_err(Error::database)?;
        tx.commit().map_err(Error::database)?;
        Ok(Self(db))
    }

    /// Every allowed peer.
    pub(super) fn allowed(&self) -> Result<HashSet<PeerId>, Error> {
        let tx = self.0.begin_read().map_err(Error::database)?;
        let table = tx.open_table(ALLOWED).map_err(Error::database)?;
        let rows = table.iter().map_err(Error::database)?;
        rows.map(|row| row.map(|(peer, _)| PeerId::from(*peer.value())))
            .collect::<Result<_, _>>()
            .map_err(Error::database)
    }

    /// Saves `peer` as allowed or not allowed.
    pub(super) fn set_allowed(&self, peer: PeerId, allowed: bool) -> Result<(), Error> {
        let tx = self.0.begin_write().map_err(Error::database)?;
        {
            let mut table = tx.open_table(ALLOWED).map_err(Error::database)?;
            if allowed {
                table.insert(peer.as_ref(), ()).map_err(Error::database)?;
            } else {
                table.remove(peer.as_ref()).map_err(Error::database)?;
            }
        }
        tx.commit().map_err(Error::database)
    }

    /// Saves a newly issued invite token, and deletes the used and expired
    /// ones: they would be refused anyway, so keeping them only grows the table.
    pub(super) fn add_invite(&self, token: InviteToken, issued_at: u64) -> Result<(), Error> {
        let tx = self.0.begin_write().map_err(Error::database)?;
        {
            let mut invites = tx.open_table(INVITES).map_err(Error::database)?;
            let spent: Vec<[u8; 16]> = invites
                .iter()
                .map_err(Error::database)?
                .map(|row| row.map(|(key, value)| (*key.value(), value.value())))
                .collect::<Result<Vec<_>, _>>()
                .map_err(Error::database)?
                .into_iter()
                .filter(|(_, (at, used))| check_invite(*at, *used, issued_at).is_err())
                .map(|(key, _)| key)
                .collect();
            for key in spent {
                invites.remove(&key).map_err(Error::database)?;
            }
            invites
                .insert(token.as_bytes(), (issued_at, false))
                .map_err(Error::database)?;
        }
        tx.commit().map_err(Error::database)
    }

    /// Uses `token` to add `contact`, if the token is known, unused and not
    /// expired at `now`. Returns whether it was accepted.
    ///
    /// One write transaction: two requests with the same token cannot both pass.
    pub(super) fn redeem(
        &self,
        token: InviteToken,
        contact: &Contact,
        now: u64,
    ) -> Result<bool, Error> {
        let tx = self.0.begin_write().map_err(Error::database)?;
        {
            let mut invites = tx.open_table(INVITES).map_err(Error::database)?;
            let found = invites
                .get(token.as_bytes())
                .map_err(Error::database)?
                .map(|row| row.value());
            let Some((issued_at, used)) = found else {
                return Ok(false);
            };
            if check_invite(issued_at, used, now).is_err() {
                return Ok(false);
            }
            invites
                .insert(token.as_bytes(), (issued_at, true))
                .map_err(Error::database)?;
        }
        put_contact(&tx, contact)?;
        tx.commit().map_err(Error::database)?;
        Ok(true)
    }

    /// Saves `contact` and allows them to fetch.
    pub(super) fn add_contact(&self, contact: &Contact) -> Result<(), Error> {
        let tx = self.0.begin_write().map_err(Error::database)?;
        put_contact(&tx, contact)?;
        tx.commit().map_err(Error::database)
    }

    /// Deletes the contact, stops allowing them, takes them off every share and
    /// drops the offers still waiting for them.
    pub(super) fn remove_contact(&self, peer: PeerId) -> Result<(), Error> {
        let tx = self.0.begin_write().map_err(Error::database)?;
        {
            let mut table = tx.open_table(SHARES).map_err(Error::database)?;
            let mut changed = Vec::new();
            for row in table.iter().map_err(Error::database)? {
                let (root, share) = row.map_err(Error::database)?;
                let mut share: Share =
                    postcard::from_bytes(share.value()).map_err(Error::database)?;
                if share.recipients.contains(&peer) {
                    share.recipients.retain(|p| *p != peer);
                    changed.push((*root.value(), share));
                }
            }
            // Written after reading: the table cannot change while it is being iterated.
            for (root, share) in changed {
                let bytes = postcard::to_allocvec(&share).expect("a share always serializes");
                table
                    .insert(&root, bytes.as_slice())
                    .map_err(Error::database)?;
            }
        }
        tx.open_table(CONTACTS)
            .map_err(Error::database)?
            .remove(peer.as_ref())
            .map_err(Error::database)?;
        tx.open_table(ALLOWED)
            .map_err(Error::database)?
            .remove(peer.as_ref())
            .map_err(Error::database)?;
        {
            let mut outbox = tx.open_table(OUTBOX).map_err(Error::database)?;
            let waiting: Vec<[u8; 64]> = outbox
                .iter()
                .map_err(Error::database)?
                .filter_map(|row| row.ok().map(|(key, _)| *key.value()))
                .filter(|key| key[..32] == *peer.as_ref())
                .collect();
            for key in waiting {
                outbox.remove(&key).map_err(Error::database)?;
            }
        }
        tx.commit().map_err(Error::database)
    }

    /// Saves a share and queues an offer to each of its `share.recipients`, in
    /// one write, so a crash never leaves a share without its offers. Recipients
    /// it already had are kept, unless it was revoked: then it starts afresh, so
    /// sharing it with someone new does not reopen it for those revoked.
    /// Returns the share as saved.
    pub(super) fn add_share(&self, root: ContentHash, mut share: Share) -> Result<Share, Error> {
        let tx = self.0.begin_write().map_err(Error::database)?;
        {
            let mut outbox = tx.open_table(OUTBOX).map_err(Error::database)?;
            for peer in &share.recipients {
                outbox
                    .insert(&pair_key(*peer, root), (Mail::Offer as u8, 0, 0))
                    .map_err(Error::database)?;
            }
        }
        let share = {
            let mut table = tx.open_table(SHARES).map_err(Error::database)?;
            let old: Option<Share> = table
                .get(root.as_ref())
                .map_err(Error::database)?
                .map(|row| postcard::from_bytes(row.value()))
                .transpose()
                .map_err(Error::database)?;
            if let Some(old) = old.filter(|old| old.revoked_at.is_none()) {
                let mut recipients = old.recipients;
                for peer in share.recipients {
                    if !recipients.contains(&peer) {
                        recipients.push(peer);
                    }
                }
                share.recipients = recipients;
            }
            let bytes = postcard::to_allocvec(&share).expect("a share always serializes");
            table
                .insert(root.as_ref(), bytes.as_slice())
                .map_err(Error::database)?;
            share
        };
        tx.commit().map_err(Error::database)?;
        Ok(share)
    }

    /// Marks a share revoked at `now` and queues the news to each recipient,
    /// in place of any offer still waiting. `None` if there is no such share.
    pub(super) fn revoke(&self, root: ContentHash, now: u64) -> Result<Option<Share>, Error> {
        self.change_share(root, |share, outbox| {
            share.revoked_at = Some(now);
            for peer in &share.recipients {
                outbox
                    .insert(&pair_key(*peer, root), (Mail::Revoke as u8, 0, 0))
                    .map_err(Error::database)?;
            }
            Ok(())
        })
    }

    /// Sets or clears a share's expiry. If that makes an expired share active
    /// again at `now`, its offer is queued again for every recipient with
    /// nothing waiting, since offers to an expired share were dropped. `None`
    /// if there is no such share.
    pub(super) fn set_expiry(
        &self,
        root: ContentHash,
        expires_at: Option<u64>,
        now: u64,
    ) -> Result<Option<Share>, Error> {
        self.change_share(root, |share, outbox| {
            let was_active = share.is_active(now);
            share.expires_at = expires_at;
            if !was_active && share.is_active(now) {
                for peer in &share.recipients {
                    let key = pair_key(*peer, root);
                    if outbox.get(&key).map_err(Error::database)?.is_none() {
                        outbox
                            .insert(&key, (Mail::Offer as u8, 0, 0))
                            .map_err(Error::database)?;
                    }
                }
            }
            Ok(())
        })
    }

    /// Loads a share, lets `change` edit it (and the outbox) and saves it, in
    /// one write. `None` if there is no such share.
    fn change_share(
        &self,
        root: ContentHash,
        change: impl FnOnce(
            &mut Share,
            &mut redb::Table<&[u8; 64], (u8, u32, u64)>,
        ) -> Result<(), Error>,
    ) -> Result<Option<Share>, Error> {
        let tx = self.0.begin_write().map_err(Error::database)?;
        let share = {
            let mut table = tx.open_table(SHARES).map_err(Error::database)?;
            let found: Option<Share> = table
                .get(root.as_ref())
                .map_err(Error::database)?
                .map(|row| postcard::from_bytes(row.value()))
                .transpose()
                .map_err(Error::database)?;
            let Some(mut share) = found else {
                return Ok(None);
            };
            let mut outbox = tx.open_table(OUTBOX).map_err(Error::database)?;
            change(&mut share, &mut outbox)?;
            let bytes = postcard::to_allocvec(&share).expect("a share always serializes");
            table
                .insert(root.as_ref(), bytes.as_slice())
                .map_err(Error::database)?;
            share
        };
        tx.commit().map_err(Error::database)?;
        Ok(Some(share))
    }

    /// Deletes a share. Messages for it in the outbox stay. Returns the share
    /// as it was, or `None` if there was none.
    pub(super) fn remove_share(&self, root: ContentHash) -> Result<Option<Share>, Error> {
        let tx = self.0.begin_write().map_err(Error::database)?;
        let removed = tx
            .open_table(SHARES)
            .map_err(Error::database)?
            .remove(root.as_ref())
            .map_err(Error::database)?
            .map(|row| postcard::from_bytes(row.value()))
            .transpose()
            .map_err(Error::database)?;
        tx.commit().map_err(Error::database)?;
        Ok(removed)
    }

    /// Every share, by root hash.
    pub(super) fn shares(&self) -> Result<Vec<(ContentHash, Share)>, Error> {
        let tx = self.0.begin_read().map_err(Error::database)?;
        let table = tx.open_table(SHARES).map_err(Error::database)?;
        let rows = table.iter().map_err(Error::database)?;
        rows.map(|row| {
            let (root, share) = row.map_err(Error::database)?;
            let share = postcard::from_bytes(share.value()).map_err(Error::database)?;
            Ok((ContentHash::from(*root.value()), share))
        })
        .collect()
    }

    /// One of our shares.
    pub(super) fn share(&self, root: ContentHash) -> Result<Option<Share>, Error> {
        let tx = self.0.begin_read().map_err(Error::database)?;
        let table = tx.open_table(SHARES).map_err(Error::database)?;
        let row = table.get(root.as_ref()).map_err(Error::database)?;
        row.map(|row| postcard::from_bytes(row.value()))
            .transpose()
            .map_err(Error::database)
    }

    /// Every offer waiting in the outbox.
    pub(super) fn outbox(&self) -> Result<Vec<OutboxEntry>, Error> {
        let tx = self.0.begin_read().map_err(Error::database)?;
        let table = tx.open_table(OUTBOX).map_err(Error::database)?;
        let rows = table.iter().map_err(Error::database)?;
        rows.map(|row| {
            let (key, value) = row.map_err(Error::database)?;
            let (key, (mail, attempts, next_try)) = (key.value(), value.value());
            let (to, root) = key.split_at(32);
            Ok(OutboxEntry {
                to: PeerId::from(<[u8; 32]>::try_from(to).expect("32 bytes")),
                root: ContentHash::from(<[u8; 32]>::try_from(root).expect("32 bytes")),
                mail: Mail::from_byte(mail),
                attempts,
                next_try,
            })
        })
        .collect()
    }

    /// Takes a message out of the outbox (delivered, or never deliverable).
    pub(super) fn remove_mail(
        &self,
        to: PeerId,
        root: ContentHash,
        mail: Mail,
    ) -> Result<(), Error> {
        self.update_mail(to, root, mail, None)
    }

    /// Records a failed delivery and when to try again.
    pub(super) fn mail_failed(
        &self,
        to: PeerId,
        root: ContentHash,
        mail: Mail,
        attempts: u32,
        next_try: u64,
    ) -> Result<(), Error> {
        self.update_mail(to, root, mail, Some((attempts, next_try)))
    }

    /// Removes (`None`) or reschedules the entry for `to` and `root`, but only
    /// if it still holds `mail`: a revoke may have replaced an offer that was
    /// on its way, and the offer's result must not wipe out the revoke.
    fn update_mail(
        &self,
        to: PeerId,
        root: ContentHash,
        mail: Mail,
        retry: Option<(u32, u64)>,
    ) -> Result<(), Error> {
        let key = pair_key(to, root);
        let tx = self.0.begin_write().map_err(Error::database)?;
        {
            let mut table = tx.open_table(OUTBOX).map_err(Error::database)?;
            let stored = table
                .get(&key)
                .map_err(Error::database)?
                .map(|row| row.value().0);
            if stored == Some(mail as u8) {
                match retry {
                    None => drop(table.remove(&key).map_err(Error::database)?),
                    Some((attempts, next_try)) => {
                        table
                            .insert(&key, (mail as u8, attempts, next_try))
                            .map_err(Error::database)?;
                    }
                }
            }
        }
        tx.commit().map_err(Error::database)
    }

    /// Makes every offer waiting for `to` due now. Returns whether there were any.
    pub(super) fn offer_due_now(&self, to: PeerId) -> Result<bool, Error> {
        let waiting: Vec<OutboxEntry> = self
            .outbox()?
            .into_iter()
            .filter(|entry| entry.to == to && entry.next_try > 0)
            .collect();
        if waiting.is_empty() {
            return Ok(false);
        }
        let tx = self.0.begin_write().map_err(Error::database)?;
        {
            let mut table = tx.open_table(OUTBOX).map_err(Error::database)?;
            for entry in &waiting {
                table
                    .insert(
                        &pair_key(entry.to, entry.root),
                        (entry.mail as u8, entry.attempts, 0),
                    )
                    .map_err(Error::database)?;
            }
        }
        tx.commit().map_err(Error::database)?;
        Ok(true)
    }

    /// Whether `peer` is a contact.
    pub(super) fn is_contact(&self, peer: PeerId) -> Result<bool, Error> {
        let tx = self.0.begin_read().map_err(Error::database)?;
        let table = tx.open_table(CONTACTS).map_err(Error::database)?;
        Ok(table.get(peer.as_ref()).map_err(Error::database)?.is_some())
    }

    /// Saves an incoming offer. An offer already in the inbox takes the new
    /// details (title, expiry…) but keeps its state, so offering a declined
    /// share again does not bring it back. Only a revoked one becomes new
    /// again: the sender shared it once more.
    pub(super) fn receive_offer(&self, offer: &Offer) -> Result<Offer, Error> {
        let key = pair_key(offer.from, offer.root);
        let tx = self.0.begin_write().map_err(Error::database)?;
        let saved = {
            let mut table = tx.open_table(INBOX).map_err(Error::database)?;
            let old: Option<Offer> = table
                .get(&key)
                .map_err(Error::database)?
                .map(|row| postcard::from_bytes(row.value()))
                .transpose()
                .map_err(Error::database)?;
            let state = match old.map(|old| old.state) {
                None | Some(OfferState::Revoked) => OfferState::New,
                Some(state) => state,
            };
            let offer = Offer {
                state,
                ..offer.clone()
            };
            let bytes = postcard::to_allocvec(&offer).expect("an offer always serializes");
            table
                .insert(&key, bytes.as_slice())
                .map_err(Error::database)?;
            offer
        };
        tx.commit().map_err(Error::database)?;
        Ok(saved)
    }

    /// Marks an offer from `from` revoked, unless it was already saved.
    /// Returns false if `from` never offered `root`.
    pub(super) fn revoke_offer(&self, from: PeerId, root: ContentHash) -> Result<bool, Error> {
        let key = pair_key(from, root);
        let tx = self.0.begin_write().map_err(Error::database)?;
        {
            let mut table = tx.open_table(INBOX).map_err(Error::database)?;
            let found: Option<Offer> = table
                .get(&key)
                .map_err(Error::database)?
                .map(|row| postcard::from_bytes(row.value()))
                .transpose()
                .map_err(Error::database)?;
            let Some(mut offer) = found else {
                return Ok(false);
            };
            // Files already saved stay saved; only what was not taken yet is withdrawn.
            if offer.state != OfferState::Done {
                offer.state = OfferState::Revoked;
                let bytes = postcard::to_allocvec(&offer).expect("an offer always serializes");
                table
                    .insert(&key, bytes.as_slice())
                    .map_err(Error::database)?;
            }
        }
        tx.commit().map_err(Error::database)?;
        Ok(true)
    }

    /// Deletes an offer from the inbox. Returns false if there was none.
    pub(super) fn remove_offer(&self, from: PeerId, root: ContentHash) -> Result<bool, Error> {
        let tx = self.0.begin_write().map_err(Error::database)?;
        let found = tx
            .open_table(INBOX)
            .map_err(Error::database)?
            .remove(&pair_key(from, root))
            .map_err(Error::database)?
            .is_some();
        tx.commit().map_err(Error::database)?;
        Ok(found)
    }

    /// Every offer in the inbox.
    pub(super) fn inbox(&self) -> Result<Vec<Offer>, Error> {
        let tx = self.0.begin_read().map_err(Error::database)?;
        let table = tx.open_table(INBOX).map_err(Error::database)?;
        let rows = table.iter().map_err(Error::database)?;
        rows.map(|row| {
            let (_, offer) = row.map_err(Error::database)?;
            postcard::from_bytes(offer.value()).map_err(Error::database)
        })
        .collect()
    }

    /// Changes an offer's state. Returns false if there is no such offer.
    pub(super) fn set_offer_state(
        &self,
        from: PeerId,
        root: ContentHash,
        state: OfferState,
    ) -> Result<bool, Error> {
        let key = pair_key(from, root);
        let tx = self.0.begin_write().map_err(Error::database)?;
        {
            let mut table = tx.open_table(INBOX).map_err(Error::database)?;
            let found: Option<Offer> = table
                .get(&key)
                .map_err(Error::database)?
                .map(|row| postcard::from_bytes(row.value()))
                .transpose()
                .map_err(Error::database)?;
            let Some(mut offer) = found else {
                return Ok(false);
            };
            offer.state = state;
            let bytes = postcard::to_allocvec(&offer).expect("an offer always serializes");
            table
                .insert(&key, bytes.as_slice())
                .map_err(Error::database)?;
        }
        tx.commit().map_err(Error::database)?;
        Ok(true)
    }

    /// Every contact.
    pub(super) fn contacts(&self) -> Result<Vec<Contact>, Error> {
        let tx = self.0.begin_read().map_err(Error::database)?;
        let table = tx.open_table(CONTACTS).map_err(Error::database)?;
        let rows = table.iter().map_err(Error::database)?;
        rows.map(|row| {
            let (_, contact) = row.map_err(Error::database)?;
            postcard::from_bytes(contact.value()).map_err(Error::database)
        })
        .collect()
    }
}

/// Writes `contact` and its allow-list entry inside `tx`.
fn put_contact(tx: &WriteTransaction, contact: &Contact) -> Result<(), Error> {
    let bytes = postcard::to_allocvec(contact).expect("a contact always serializes");
    tx.open_table(CONTACTS)
        .map_err(Error::database)?
        .insert(contact.peer.as_ref(), bytes.as_slice())
        .map_err(Error::database)?;
    tx.open_table(ALLOWED)
        .map_err(Error::database)?
        .insert(contact.peer.as_ref(), ())
        .map_err(Error::database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn allowed_peers_are_saved() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("caraxyn.redb");
        let (one, two) = (PeerId::from([1; 32]), PeerId::from([2; 32]));

        let db = Db::open(&path).unwrap();
        assert!(db.allowed().unwrap().is_empty());
        db.set_allowed(one, true).unwrap();
        db.set_allowed(two, true).unwrap();
        db.set_allowed(one, false).unwrap();
        drop(db);

        assert_eq!(
            Db::open(&path).unwrap().allowed().unwrap(),
            HashSet::from([two])
        );
    }

    fn contact(byte: u8) -> Contact {
        Contact {
            peer: PeerId::from([byte; 32]),
            name: format!("peer {byte}"),
            added_at: 5,
        }
    }

    #[test]
    fn an_invite_works_once_and_only_for_24_hours() {
        let dir = TempDir::new().unwrap();
        let db = Db::open(&dir.path().join("caraxyn.redb")).unwrap();
        let (fresh, old) = (InviteToken::random(), InviteToken::random());
        db.add_invite(fresh, 1_000).unwrap();
        db.add_invite(old, 1_000).unwrap();
        let day = crate::contacts::INVITE_TTL_MS;

        assert!(
            !db.redeem(old, &contact(1), 1_000 + day).unwrap(),
            "expired"
        );
        assert!(db.redeem(fresh, &contact(2), 1_000 + day - 1).unwrap());
        assert!(!db.redeem(fresh, &contact(3), 1_000).unwrap(), "used");
        assert!(
            !db.redeem(InviteToken::random(), &contact(4), 1_000)
                .unwrap(),
            "unknown"
        );

        assert_eq!(db.contacts().unwrap(), vec![contact(2)]);
        assert_eq!(db.allowed().unwrap(), HashSet::from([contact(2).peer]));
    }

    #[test]
    fn a_new_invite_clears_out_used_and_expired_ones() {
        let dir = TempDir::new().unwrap();
        let db = Db::open(&dir.path().join("caraxyn.redb")).unwrap();
        let day = crate::contacts::INVITE_TTL_MS;
        let (used, expired, valid) = (
            InviteToken::random(),
            InviteToken::random(),
            InviteToken::random(),
        );
        db.add_invite(used, day).unwrap();
        db.add_invite(expired, 0).unwrap();
        assert!(db.redeem(used, &contact(1), day).unwrap());
        db.add_invite(valid, day + 1).unwrap();

        db.add_invite(InviteToken::random(), day + 2).unwrap();

        let tx = db.0.begin_read().unwrap();
        let left = tx.open_table(INVITES).unwrap().iter().unwrap().count();
        assert_eq!(left, 2, "the valid one and the newest");
        assert!(db.redeem(valid, &contact(2), day + 3).unwrap());
    }

    #[test]
    fn contacts_are_saved_and_removed_with_their_access() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("caraxyn.redb");
        let db = Db::open(&path).unwrap();
        db.add_contact(&contact(1)).unwrap();
        db.add_contact(&contact(2)).unwrap();
        db.remove_contact(contact(1).peer).unwrap();
        drop(db);

        let db = Db::open(&path).unwrap();
        assert_eq!(db.contacts().unwrap(), vec![contact(2)]);
        assert_eq!(db.allowed().unwrap(), HashSet::from([contact(2).peer]));
    }

    #[test]
    fn sharing_again_adds_recipients() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("caraxyn.redb");
        let root = ContentHash::from([9; 32]);
        let (bob, carol) = (PeerId::from([1; 32]), PeerId::from([2; 32]));
        let hashes = vec![root, ContentHash::from([8; 32])];

        let db = Db::open(&path).unwrap();
        let share = |recipients| Share {
            recipients,
            hashes: hashes.clone(),
            ..Share::default()
        };
        db.add_share(root, share(vec![bob])).unwrap();
        db.add_share(root, share(vec![carol, bob])).unwrap();
        drop(db);

        let share = share(vec![bob, carol]);
        assert_eq!(
            Db::open(&path).unwrap().shares().unwrap(),
            vec![(root, share)]
        );
    }

    #[test]
    fn removing_a_contact_takes_them_off_every_share() {
        let dir = TempDir::new().unwrap();
        let db = Db::open(&dir.path().join("caraxyn.redb")).unwrap();
        let (bob, carol) = (contact(1), contact(2));
        db.add_contact(&bob).unwrap();
        let (one, two) = (ContentHash::from([7; 32]), ContentHash::from([8; 32]));
        let share = |recipients, root| Share {
            recipients,
            hashes: vec![root],
            ..Share::default()
        };
        db.add_share(one, share(vec![bob.peer, carol.peer], one))
            .unwrap();
        db.add_share(two, share(vec![bob.peer], two)).unwrap();

        db.remove_contact(bob.peer).unwrap();

        let recipients: Vec<_> = db
            .shares()
            .unwrap()
            .into_iter()
            .map(|(root, share)| (root, share.recipients))
            .collect();
        assert_eq!(recipients, vec![(one, vec![carol.peer]), (two, vec![])]);
    }

    fn offer(from: u8, root: u8) -> Offer {
        Offer {
            from: PeerId::from([from; 32]),
            root: ContentHash::from([root; 32]),
            title: "trip".into(),
            file_count: 1,
            total_size: 2,
            preview: vec![("trip/a.jpg".into(), 2)],
            expires_at: None,
            received_at: 3,
            state: OfferState::New,
        }
    }

    #[test]
    fn a_repeated_offer_keeps_its_state() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("caraxyn.redb");
        let db = Db::open(&path).unwrap();
        db.receive_offer(&offer(1, 9)).unwrap();
        db.receive_offer(&offer(2, 9)).unwrap();
        assert!(
            db.set_offer_state(offer(1, 9).from, offer(1, 9).root, OfferState::Declined)
                .unwrap()
        );
        assert!(
            !db.set_offer_state(PeerId::from([3; 32]), offer(1, 9).root, OfferState::Done)
                .unwrap()
        );

        db.receive_offer(&offer(1, 9)).unwrap();
        drop(db);

        let inbox = Db::open(&path).unwrap().inbox().unwrap();
        let states: Vec<_> = inbox.iter().map(|o| (o.from, o.state)).collect();
        assert_eq!(
            states,
            vec![
                (offer(1, 9).from, OfferState::Declined),
                (offer(2, 9).from, OfferState::New)
            ]
        );
    }

    #[test]
    fn an_offer_finishing_late_does_not_wipe_out_the_revoke_that_replaced_it() {
        let dir = TempDir::new().unwrap();
        let db = Db::open(&dir.path().join("caraxyn.redb")).unwrap();
        let (bob, root) = (PeerId::from([1; 32]), ContentHash::from([9; 32]));
        let share = Share {
            recipients: vec![bob],
            hashes: vec![root],
            ..Share::default()
        };
        db.add_share(root, share).unwrap();

        // The offer is on its way when the share is revoked...
        db.revoke(root, 5).unwrap();
        // ...and its result arrives afterwards.
        db.remove_mail(bob, root, Mail::Offer).unwrap();
        db.mail_failed(bob, root, Mail::Offer, 1, 99).unwrap();

        let outbox = db.outbox().unwrap();
        assert_eq!(outbox.len(), 1);
        assert_eq!((outbox[0].mail, outbox[0].attempts), (Mail::Revoke, 0));
        assert!(!db.share(root).unwrap().unwrap().is_active(6));
    }

    #[test]
    fn a_resent_offer_brings_new_details_and_revives_only_a_revoked_one() {
        let dir = TempDir::new().unwrap();
        let db = Db::open(&dir.path().join("caraxyn.redb")).unwrap();
        let (revoked, declined) = (offer(1, 9), offer(2, 9));
        db.receive_offer(&revoked).unwrap();
        db.receive_offer(&declined).unwrap();
        assert!(db.revoke_offer(revoked.from, revoked.root).unwrap());
        assert!(
            db.set_offer_state(declined.from, declined.root, OfferState::Declined)
                .unwrap()
        );

        for old in [&revoked, &declined] {
            db.receive_offer(&Offer {
                expires_at: Some(42),
                ..old.clone()
            })
            .unwrap();
        }

        let inbox = db.inbox().unwrap();
        let found: Vec<_> = inbox
            .iter()
            .map(|o| (o.from, o.state, o.expires_at))
            .collect();
        assert_eq!(
            found,
            vec![
                (revoked.from, OfferState::New, Some(42)),
                (declined.from, OfferState::Declined, Some(42)),
            ]
        );
    }

    #[test]
    fn a_data_folder_in_another_format_is_refused_untouched() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("caraxyn.redb");
        // A new folder gets the current format and opens again.
        drop(Db::open(&path).unwrap());
        drop(Db::open(&path).unwrap());

        // A newer (or older) numbered format.
        {
            let db = Database::create(&path).unwrap();
            let tx = db.begin_write().unwrap();
            tx.open_table(META).unwrap().insert("format", 99).unwrap();
            tx.commit().unwrap();
        }
        let newer = Db::open(&path);
        assert!(
            matches!(
                newer,
                Err(Error::IncompatibleData {
                    found: 99,
                    expected: FORMAT
                })
            ),
            "got {newer:?}"
        );

        // From before formats were numbered: tables, but no number.
        let old = dir.path().join("old.redb");
        {
            let db = Database::create(&old).unwrap();
            let tx = db.begin_write().unwrap();
            tx.open_table(ALLOWED)
                .unwrap()
                .insert(&[1; 32], ())
                .unwrap();
            tx.commit().unwrap();
        }
        let older = Db::open(&old);
        assert!(
            matches!(older, Err(Error::IncompatibleData { found: 0, .. })),
            "got {older:?}"
        );
        // Untouched: still no format number written.
        let db = Database::create(&old).unwrap();
        let tx = db.begin_read().unwrap();
        assert!(tx.open_table(META).is_err());
    }
}
