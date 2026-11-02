//! A running Caraxyn node.
//!
//! This is the only module that talks to iroh directly; everything else
//! sees Caraxyn's own types.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
};

use iroh::{
    Endpoint, EndpointId,
    address_lookup::MemoryLookup,
    endpoint::presets::{self, Preset},
    protocol::Router,
};
use iroh_blobs::{
    BlobFormat, BlobsProtocol, Hash, HashAndFormat,
    api::{
        Store,
        blobs::{AddPathOptions, ImportMode},
        downloader::Shuffled,
    },
    format::collection::Collection,
    provider::events::{AbortReason, ConnectMode, EventMask, EventSender, ProviderMessage},
    store::{
        GcConfig,
        fs::{FsStore, options::Options},
    },
};
use tracing::{info, warn};

use crate::{
    error::Error,
    ids::{ContentHash, PeerId},
    safe_name,
};

/// How often the blob store deletes data no tag protects.
const GC_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Tag that keeps one of our own shares (and every file in it) in the store.
fn share_tag(root: ContentHash) -> String {
    format!("share/{root}")
}

/// Tag that keeps a received share in the store until it has been exported.
fn recv_tag(root: ContentHash) -> String {
    format!("recv/{root}")
}

impl From<EndpointId> for PeerId {
    fn from(id: EndpointId) -> Self {
        Self::from(*id.as_bytes())
    }
}

impl From<Hash> for ContentHash {
    fn from(hash: Hash) -> Self {
        Self::from(*hash.as_bytes())
    }
}

impl From<ContentHash> for Hash {
    fn from(hash: ContentHash) -> Self {
        Self::from(*hash.as_ref())
    }
}

/// Settings for [`Node::spawn`].
#[derive(Clone, Debug)]
pub struct NodeConfig {
    /// Directory where the node keeps its data: the blob store and the
    /// secret key that is its identity.
    pub data_dir: PathBuf,
}

impl NodeConfig {
    /// Config with the given data directory.
    #[must_use]
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }
}

/// Handle to a running node. Cloning is cheap and shares the same node.
#[derive(Clone, Debug)]
pub struct Node {
    /// Owns the endpoint and answers incoming blob requests from the store.
    router: Router,
    store: Store,
    /// Peers allowed to fetch from us; everyone else is refused on connect.
    /// A copy of the database's list, so the check never reads the disk.
    allowed: Arc<RwLock<HashSet<PeerId>>>,
    db: Arc<db::Db>,
}

impl Node {
    /// Starts a node, using iroh's default discovery and relay servers.
    ///
    /// The identity is loaded from `data_dir/secret.key`, or created there on
    /// the first start, so the [`PeerId`] stays the same across restarts.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Bind`] if the network endpoint cannot be started,
    /// [`Error::Store`] if the blob store cannot be opened,
    /// [`Error::InvalidKeyFile`] if the key file is damaged, [`Error::Io`]
    /// if it cannot be read or written, or [`Error::Database`] if the
    /// database cannot be opened.
    pub async fn spawn(config: NodeConfig) -> Result<Self, Error> {
        Self::spawn_with(config, presets::N0, None, GC_INTERVAL).await
    }

    /// `lookup` and a short `gc_interval` are only used by tests: to let nodes
    /// find each other offline, and to see garbage collection happen.
    async fn spawn_with(
        config: NodeConfig,
        preset: impl Preset,
        lookup: Option<MemoryLookup>,
        gc_interval: Duration,
    ) -> Result<Self, Error> {
        let blobs = config.data_dir.join("blobs");
        let mut options = Options::new(&blobs);
        // Deletes every blob no tag reaches. It never deletes a file shared in
        // place: only the store's own copies and its record of the file.
        options.gc = Some(GcConfig {
            interval: gc_interval,
            add_protected: None,
        });
        let store: Store = FsStore::load_with_opts(blobs.join("blobs.db"), options)
            .await
            .map_err(Error::store)?
            .into();
        // After the store: it locks `data_dir`, so two nodes never race to create the key.
        let key = key::load_or_create(&config.data_dir.join("secret.key"))?;
        let db = db::Db::open(&config.data_dir.join("caraxyn.redb"))?;
        let allowed = Arc::new(RwLock::new(db.allowed()?));
        let mut builder = Endpoint::builder(preset).secret_key(key);
        if let Some(lookup) = lookup {
            builder = builder.address_lookup(lookup);
        }
        let endpoint = builder
            .bind()
            .await
            .map_err(|e| Error::Bind(e.to_string()))?;
        let (events, mut requests) = EventSender::channel(
            16,
            EventMask {
                connected: ConnectMode::Intercept,
                ..EventMask::DEFAULT
            },
        );
        let gate = Arc::clone(&allowed);
        // Ends by itself when the router (and with it the sender) is dropped.
        tokio::spawn(async move {
            while let Some(msg) = requests.recv().await {
                if let ProviderMessage::ClientConnected(m) = msg {
                    let ok = m.endpoint_id.is_some_and(|id| {
                        gate.read()
                            .expect("allow-list lock poisoned")
                            .contains(&PeerId::from(id))
                    });
                    let verdict = if ok {
                        Ok(())
                    } else {
                        Err(AbortReason::Permission)
                    };
                    m.tx.send(verdict).await.ok();
                }
            }
        });
        let router = Router::builder(endpoint)
            .accept(iroh_blobs::ALPN, BlobsProtocol::new(&store, Some(events)))
            .accept(control::CONTROL_ALPN, control::ControlHandler)
            .spawn();
        let node = Self {
            router,
            store,
            allowed,
            db: Arc::new(db),
        };
        info!(peer = %node.peer_id().short(), "node started");
        Ok(node)
    }

    /// This node's identity.
    #[must_use]
    pub fn peer_id(&self) -> PeerId {
        self.router.endpoint().id().into()
    }

    /// Lets `peer` fetch from this node. Nobody is allowed by default.
    ///
    /// The list is saved, so it survives restarts. It is checked when a peer
    /// connects, so every share on this node is open to every allowed peer.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] if the change cannot be saved; nothing changes then.
    pub fn allow(&self, peer: PeerId) -> Result<(), Error> {
        self.set_allowed(peer, true)
    }

    /// Stops `peer` from connecting again. A transfer already running is not cut.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] if the change cannot be saved; nothing changes then.
    pub fn disallow(&self, peer: PeerId) -> Result<(), Error> {
        self.set_allowed(peer, false)
    }

    fn set_allowed(&self, peer: PeerId, allowed: bool) -> Result<(), Error> {
        // Lock held across the save, so two calls cannot leave memory and disk disagreeing.
        let mut list = self.allowed.write().expect("allow-list lock poisoned");
        // Disk first: if saving fails, memory still matches what is saved.
        self.db.set_allowed(peer, allowed)?;
        if allowed {
            list.insert(peer);
        } else {
            list.remove(&peer);
        }
        Ok(())
    }

    /// Sends one control message to `peer` and waits for its answer.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by the contact flow (step C)")
    )]
    async fn request(
        &self,
        peer: PeerId,
        msg: &control::ControlMsg,
    ) -> Result<control::ControlMsg, Error> {
        let id = EndpointId::from_bytes(peer.as_ref()).map_err(|_| Error::InvalidPeerId)?;
        let failed = |e: &dyn std::fmt::Display| Error::Control(e.to_string());
        let conn = self
            .router
            .endpoint()
            .connect(id, control::CONTROL_ALPN)
            .await
            .map_err(|e| failed(&e))?;
        let (mut tx, mut rx) = conn.open_bi().await.map_err(|e| failed(&e))?;
        control::send(&mut tx, msg).await.map_err(|e| failed(&e))?;
        let answer = control::recv(&mut rx).await.map_err(|e| failed(&e));
        conn.close(0u32.into(), b"done");
        answer
    }

    /// Downloads a share from another node into our own store.
    ///
    /// Everything is verified against its hash while it streams in. Returns
    /// once the whole collection is stored. The share stays in the store,
    /// across restarts too, until it is [exported](Self::export); a failed
    /// download keeps what it verified so far, and a retry continues from there.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidPeerId`] if `from` is not a valid key, [`Error::Store`]
    /// if the peer is unreachable, does not have the share, refuses us, or sends
    /// bad data.
    // shortcut: iroh reports a refusal as a plain failed download, so it cannot be told apart yet; add Error::Refused if a caller needs it.
    pub async fn fetch(&self, from: PeerId, root: ContentHash) -> Result<(), Error> {
        let provider = EndpointId::from_bytes(from.as_ref()).map_err(|_| Error::InvalidPeerId)?;
        // Before the download, so garbage collection cannot delete half of it.
        self.store
            .tags()
            .set(recv_tag(root), HashAndFormat::hash_seq(root.into()))
            .await
            .map_err(Error::store)?;
        let download = self
            .store
            .downloader(self.router.endpoint())
            .download(
                HashAndFormat::hash_seq(root.into()),
                Shuffled::new(vec![provider]),
            )
            .await;
        if let Err(e) = download {
            // The tag stays when the share's file list arrived, so a retry resumes.
            // Without the list there is nothing to resume, and the tag would only
            // make every garbage collection run log a warning. If the check itself
            // fails, keep the tag: a leftover tag is harmless, lost data is not.
            if !self
                .store
                .blobs()
                .has(Hash::from(root))
                .await
                .unwrap_or(true)
                && let Err(e) = self.store.tags().delete(recv_tag(root)).await
            {
                warn!(share = %root.short(), %e, "could not drop the tag of a failed fetch");
            }
            return Err(Error::store(e));
        }
        info!(from = %from.short(), share = %root.short(), "share fetched");
        Ok(())
    }

    /// Imports a file so it can be shared, and returns the hash of the share.
    ///
    /// The file is referenced in place rather than copied: it must not be
    /// changed, moved or deleted while it is shared. The share is a collection
    /// holding one entry named after the file (never its full path).
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file does not exist, [`Error::InvalidPath`] if it
    /// has no usable name, [`Error::UnsafeName`] if a receiver could not save
    /// it under that name, [`Error::Store`] if the import fails.
    pub async fn share_file(&self, path: &Path) -> Result<ContentHash, Error> {
        // The store requires an absolute path; this also fails if the file is missing.
        let path = tokio::fs::canonicalize(path)
            .await
            .map_err(Error::io(path))?;
        let name = file_name(&path)?;
        self.share_entries(vec![(name, path)]).await
    }

    /// Imports a folder so it can be shared, and returns the hash of the share.
    ///
    /// Like [`share_file`](Self::share_file), files are referenced in place.
    /// The share has one entry per file, named by its path relative to the
    /// folder's parent (`photos/trip/a.jpg`), in sorted order so the same folder
    /// always gives the same hash. Symlinks are skipped, hidden files are
    /// included, and empty folders are not shared.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the folder or any file in it cannot be read,
    /// [`Error::InvalidPath`] if a name is not UTF-8, [`Error::UnsafeName`] if
    /// a receiver could not save a name, [`Error::Store`] if an import fails.
    // shortcut: a collection only lists files, so empty folders are lost; needs a marker format to fix.
    pub async fn share_folder(&self, path: &Path) -> Result<ContentHash, Error> {
        let root = tokio::fs::canonicalize(path)
            .await
            .map_err(Error::io(path))?;
        let mut entries = Vec::new();
        let mut pending = vec![(file_name(&root)?, root)];
        while let Some((name, dir)) = pending.pop() {
            let mut read = tokio::fs::read_dir(&dir).await.map_err(Error::io(&dir))?;
            while let Some(entry) = read.next_entry().await.map_err(Error::io(&dir))? {
                let child = format!("{name}/{}", file_name(&entry.path())?);
                let kind = entry.file_type().await.map_err(Error::io(&entry.path()))?;
                if kind.is_dir() {
                    pending.push((child, entry.path()));
                } else if kind.is_file() {
                    entries.push((child, entry.path()));
                } else {
                    warn!(path = %entry.path().display(), "skipping symlink or special file");
                }
            }
        }
        entries.sort();
        self.share_entries(entries).await
    }

    /// Imports each `(name, path)` and stores them as one tagged collection.
    async fn share_entries(&self, entries: Vec<(String, PathBuf)>) -> Result<ContentHash, Error> {
        // Refuse here what the receiver's export would refuse, before importing anything.
        for (name, _) in &entries {
            check_name(name)?;
        }
        let mut files = Vec::new();
        // Temp tags keep the blobs alive until the collection references them.
        let mut tags = Vec::new();
        for (name, path) in entries {
            let tag = self
                .store
                .blobs()
                .add_path_with_opts(AddPathOptions {
                    path,
                    mode: ImportMode::TryReference,
                    format: BlobFormat::Raw,
                })
                .temp_tag()
                .await
                .map_err(Error::store)?;
            files.push((name, tag.hash()));
            tags.push(tag);
        }
        let root = Collection::from_iter(files)
            .store(&self.store)
            .await
            .map_err(Error::store)?;
        let root_hash = root.hash().into();
        // A named tag, so sharing the same thing again reuses it instead of adding one.
        self.store
            .tags()
            .set(share_tag(root_hash), root.hash_and_format())
            .await
            .map_err(Error::store)?;
        Ok(root_hash)
    }

    /// Writes a share held in our store into the existing folder `dest`.
    ///
    /// Every name is checked, component by component, and nothing is
    /// overwritten. Folders are created as needed. If anything fails, `dest`
    /// is left as it was. Returns the paths written.
    ///
    /// After a successful export of a received share, its copy in the store is
    /// freed at the next garbage collection; exporting it again needs a new
    /// [`fetch`](Self::fetch).
    ///
    /// # Errors
    ///
    /// [`Error::UnsafeName`] if the sender chose a name that is not safe,
    /// [`Error::AlreadyExists`] if the share's top-level file or folder is
    /// already in `dest`, [`Error::Io`] if `dest` is not a folder,
    /// [`Error::Store`] if the share is not (fully) in the store.
    pub async fn export(&self, root: ContentHash, dest: &Path) -> Result<Vec<PathBuf>, Error> {
        // iroh's export needs an absolute path. Not canonicalize: on Windows it
        // gives `\\?\C:\...` paths, which would end up in the returned paths.
        let dest = &std::path::absolute(dest).map_err(Error::io(dest))?;
        let meta = tokio::fs::metadata(dest).await.map_err(Error::io(dest))?;
        if !meta.is_dir() {
            return Err(Error::io(dest)(std::io::ErrorKind::NotADirectory.into()));
        }
        let collection = Collection::load(root.into(), &self.store)
            .await
            .map_err(Error::store)?;

        let mut targets = Vec::new();
        let mut files = HashSet::new();
        let mut dirs = HashSet::new();
        let mut top_level = HashSet::new();
        for (name, hash) in collection.iter() {
            check_name(name)?;
            let unsafe_name = |reason| Error::UnsafeName {
                name: name.clone(),
                reason,
            };
            // shortcut: exact-match only, so names differing in case clash on case-insensitive filesystems; upgrade when that bites.
            let mut clash = dirs.contains(name.as_str()) || !files.insert(name.as_str());
            for (i, _) in name.match_indices('/') {
                clash |= files.contains(&name[..i]);
                dirs.insert(&name[..i]);
            }
            if clash {
                return Err(unsafe_name(safe_name::NameError::Conflict));
            }
            top_level.insert(name.split('/').next().unwrap_or(name));
            // Part by part, so each piece is joined with the platform's own separator.
            let path = name
                .split('/')
                .fold(dest.to_path_buf(), |p, part| p.join(part));
            targets.push((*hash, path));
        }
        let top_level: Vec<PathBuf> = top_level.into_iter().map(|n| dest.join(n)).collect();
        for path in &top_level {
            // shortcut: check-then-write is racy; fine for a single-user desktop app.
            if tokio::fs::try_exists(path).await.map_err(Error::io(path))? {
                return Err(Error::AlreadyExists(path.clone()));
            }
        }

        let mut written = Vec::new();
        for (hash, path) in targets {
            if let Err(e) = self.export_one(hash, &path).await {
                // Nothing at these paths existed before (checked above), so remove it all.
                for top in &top_level {
                    let removed = if top.is_dir() {
                        tokio::fs::remove_dir_all(top).await
                    } else {
                        tokio::fs::remove_file(top).await
                    };
                    // NotFound: the write failed before reaching this entry.
                    if let Err(cleanup) = removed
                        && cleanup.kind() != std::io::ErrorKind::NotFound
                    {
                        warn!(path = %top.display(), %cleanup, "could not clean up after a failed export");
                    }
                }
                return Err(e);
            }
            written.push(path);
        }
        // Saved, so the store copy may go. Not an error: the files are already written.
        if let Err(e) = self.store.tags().delete(recv_tag(root)).await {
            warn!(share = %root.short(), %e, "could not release the received share");
        }
        info!(share = %root.short(), files = written.len(), "share exported");
        Ok(written)
    }

    /// Writes one blob to `path`, creating its parent folders.
    ///
    /// Copies rather than moves: moving would leave the store pointing at a
    /// file the user may later delete. The store copy is freed by garbage
    /// collection once the share's `recv/` tag is gone.
    async fn export_one(&self, hash: Hash, path: &Path) -> Result<(), Error> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(Error::io(parent))?;
        }
        self.store
            .blobs()
            .export(hash, path)
            .await
            .map_err(Error::store)?;
        Ok(())
    }

    /// Stops the node: closes connections and the blob store.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Store`] if the node does not shut down cleanly.
    pub async fn shutdown(self) -> Result<(), Error> {
        // The router also shuts down the blob store (via the blobs protocol).
        self.router.shutdown().await.map_err(Error::store)?;
        info!(peer = %self.peer_id().short(), "node stopped");
        Ok(())
    }
}

/// Checks every `/`-separated part of a share entry name.
fn check_name(name: &str) -> Result<(), Error> {
    for part in name.split('/') {
        safe_name::check(part).map_err(|reason| Error::UnsafeName {
            name: name.to_owned(),
            reason,
        })?;
    }
    Ok(())
}

/// The last path component as UTF-8.
fn file_name(path: &Path) -> Result<String, Error> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .ok_or(Error::InvalidPath)
}

mod control;
mod db;
mod key;

#[cfg(test)]
mod tests;
