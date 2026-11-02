//! Caraxyn's own small database, kept in one file next to the blob store.

use std::{collections::HashSet, path::Path};

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::{error::Error, ids::PeerId};

/// Peers allowed to fetch from us. Key: the peer's id; no value yet.
const ALLOWED: TableDefinition<&[u8; 32], ()> = TableDefinition::new("allowed_peers");

/// Handle to the database file.
#[derive(Debug)]
pub(super) struct Db(Database);

// shortcut: writes run on the caller's thread (a few ms each, fine for an allow-list); move them off the async runtime when tables get busy.
impl Db {
    /// Opens the database at `path`, creating the file and its tables if needed.
    pub(super) fn open(path: &Path) -> Result<Self, Error> {
        let db = Database::create(path).map_err(Error::database)?;
        // Creates the table on first open, so reads never find it missing.
        let tx = db.begin_write().map_err(Error::database)?;
        tx.open_table(ALLOWED).map_err(Error::database)?;
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
}
