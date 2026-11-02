//! Caraxyn's own small database, kept in one file next to the blob store.

use std::{collections::HashSet, path::Path};

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition, WriteTransaction};

use crate::{
    contacts::{Contact, InviteToken, check_invite},
    error::Error,
    ids::{ContentHash, PeerId},
};

use serde::{Deserialize, Serialize};

/// Peers allowed to fetch from us. Key: the peer's id; no value yet.
const ALLOWED: TableDefinition<&[u8; 32], ()> = TableDefinition::new("allowed_peers");

/// Invites we issued. Key: the token; value: when it was issued (unix ms) and whether it was used.
const INVITES: TableDefinition<&[u8; 16], (u64, bool)> = TableDefinition::new("invites");

/// Our contacts. Key: the peer's id; value: the [`Contact`] in postcard.
const CONTACTS: TableDefinition<&[u8; 32], &[u8]> = TableDefinition::new("contacts");

/// Our shares. Key: the share's root hash; value: the [`Share`] in postcard.
const SHARES: TableDefinition<&[u8; 32], &[u8]> = TableDefinition::new("shares");

/// Who a share is for, and every hash it is made of.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Share {
    /// Peers allowed to fetch it.
    pub(super) recipients: Vec<PeerId>,
    /// The root, its list of names and every file: all a recipient may ask for.
    pub(super) hashes: Vec<ContentHash>,
}

/// Handle to the database file.
#[derive(Debug)]
pub(super) struct Db(Database);

// shortcut: writes run on the caller's thread (a few ms each, fine for an allow-list); move them off the async runtime when tables get busy.
impl Db {
    /// Opens the database at `path`, creating the file and its tables if needed.
    pub(super) fn open(path: &Path) -> Result<Self, Error> {
        let db = Database::create(path).map_err(Error::database)?;
        // Creates the tables on first open, so reads never find them missing.
        let tx = db.begin_write().map_err(Error::database)?;
        tx.open_table(ALLOWED).map_err(Error::database)?;
        tx.open_table(INVITES).map_err(Error::database)?;
        tx.open_table(CONTACTS).map_err(Error::database)?;
        tx.open_table(SHARES).map_err(Error::database)?;
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

    /// Saves a newly issued invite token.
    pub(super) fn add_invite(&self, token: InviteToken, issued_at: u64) -> Result<(), Error> {
        let tx = self.0.begin_write().map_err(Error::database)?;
        tx.open_table(INVITES)
            .map_err(Error::database)?
            .insert(token.as_bytes(), (issued_at, false))
            .map_err(Error::database)?;
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

    /// Deletes the contact, stops allowing them and takes them off every share.
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
        tx.commit().map_err(Error::database)
    }

    /// Saves a share, adding `recipients` to any it already had. Returns the
    /// share as saved.
    pub(super) fn add_share(
        &self,
        root: ContentHash,
        recipients: &[PeerId],
        hashes: Vec<ContentHash>,
    ) -> Result<Share, Error> {
        let tx = self.0.begin_write().map_err(Error::database)?;
        let share = {
            let mut table = tx.open_table(SHARES).map_err(Error::database)?;
            let mut share: Share = match table.get(root.as_ref()).map_err(Error::database)? {
                Some(row) => postcard::from_bytes(row.value()).map_err(Error::database)?,
                None => Share::default(),
            };
            for peer in recipients {
                if !share.recipients.contains(peer) {
                    share.recipients.push(*peer);
                }
            }
            share.hashes = hashes;
            let bytes = postcard::to_allocvec(&share).expect("a share always serializes");
            table
                .insert(root.as_ref(), bytes.as_slice())
                .map_err(Error::database)?;
            share
        };
        tx.commit().map_err(Error::database)?;
        Ok(share)
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
        db.add_share(root, &[bob], hashes.clone()).unwrap();
        db.add_share(root, &[carol, bob], hashes.clone()).unwrap();
        drop(db);

        let share = Share {
            recipients: vec![bob, carol],
            hashes,
        };
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
        db.add_share(one, &[bob.peer, carol.peer], vec![one])
            .unwrap();
        db.add_share(two, &[bob.peer], vec![two]).unwrap();

        db.remove_contact(bob.peer).unwrap();

        let recipients: Vec<_> = db
            .shares()
            .unwrap()
            .into_iter()
            .map(|(root, share)| (root, share.recipients))
            .collect();
        assert_eq!(recipients, vec![(one, vec![carol.peer]), (two, vec![])]);
    }
}
