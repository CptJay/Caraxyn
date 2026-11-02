//! A running Caraxyn node.
//!
//! This is the only module that talks to iroh directly; everything else
//! sees Caraxyn's own types.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
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
    store::fs::FsStore,
};
use tracing::info;

use crate::{
    error::Error,
    ids::{ContentHash, PeerId},
    safe_name,
};

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
    /// Directory where the node keeps its data (the blob store).
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
    allowed: Arc<RwLock<HashSet<PeerId>>>,
}

impl Node {
    /// Starts a node with a fresh identity, using iroh's default discovery
    /// and relay servers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Bind`] if the network endpoint cannot be started, or
    /// [`Error::Store`] if the blob store cannot be opened.
    pub async fn spawn(config: NodeConfig) -> Result<Self, Error> {
        Self::spawn_with(config, presets::N0, None).await
    }

    /// `lookup` is only used by tests, to let nodes find each other offline.
    async fn spawn_with(
        config: NodeConfig,
        preset: impl Preset,
        lookup: Option<MemoryLookup>,
    ) -> Result<Self, Error> {
        let store: Store = FsStore::load(config.data_dir.join("blobs"))
            .await
            .map_err(Error::store)?
            .into();
        let mut builder = Endpoint::builder(preset);
        if let Some(lookup) = lookup {
            builder = builder.address_lookup(lookup);
        }
        let endpoint = builder.bind().await?;
        let allowed = Arc::new(RwLock::new(HashSet::new()));
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
            .spawn();
        let node = Self {
            router,
            store,
            allowed,
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
    /// The list lives in memory only. It is checked when a peer connects, so
    /// every share on this node is open to every allowed peer.
    pub fn allow(&self, peer: PeerId) {
        self.allowed
            .write()
            .expect("allow-list lock poisoned")
            .insert(peer);
    }

    /// Stops `peer` from connecting again. A transfer already running is not cut.
    pub fn disallow(&self, peer: PeerId) {
        self.allowed
            .write()
            .expect("allow-list lock poisoned")
            .remove(&peer);
    }

    /// Downloads a share from another node into our own store.
    ///
    /// Everything is verified against its hash while it streams in. Returns
    /// once the whole collection is stored.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidPeerId`] if `from` is not a valid key, [`Error::Store`]
    /// if the peer is unreachable, does not have the share, refuses us, or sends
    /// bad data.
    // shortcut: iroh reports a refusal as a plain failed download, so it cannot be told apart yet; add Error::Refused if a caller needs it.
    pub async fn fetch(&self, from: PeerId, root: ContentHash) -> Result<(), Error> {
        let provider = EndpointId::from_bytes(from.as_ref()).map_err(|_| Error::InvalidPeerId)?;
        self.store
            .downloader(self.router.endpoint())
            .download(
                HashAndFormat::hash_seq(root.into()),
                Shuffled::new(vec![provider]),
            )
            .await
            .map_err(Error::store)?;
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
    /// has no usable name, [`Error::Store`] if the import fails.
    pub async fn share_file(&self, path: &Path) -> Result<ContentHash, Error> {
        // The store requires an absolute path; this also fails if the file is missing.
        let path = tokio::fs::canonicalize(path).await?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(Error::InvalidPath)?
            .to_owned();

        let file = self
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
        let root = Collection::from_iter([(name, file.hash())])
            .store(&self.store)
            .await
            .map_err(Error::store)?;
        // A tag keeps the collection from being garbage-collected.
        self.store
            .tags()
            .create(root.hash_and_format())
            .await
            .map_err(Error::store)?;
        Ok(root.hash().into())
    }

    /// Writes a share held in our store into the existing folder `dest`.
    ///
    /// Every name is checked and nothing is overwritten before the first file
    /// is written, so a bad share leaves `dest` untouched. Returns the paths
    /// written.
    ///
    /// # Errors
    ///
    /// [`Error::UnsafeName`] if the sender chose a name that is not safe,
    /// [`Error::AlreadyExists`] if a file with that name is already in `dest`,
    /// [`Error::Io`] if `dest` is not a folder, [`Error::Store`] if the share
    /// is not (fully) in the store.
    pub async fn export(&self, root: ContentHash, dest: &Path) -> Result<Vec<PathBuf>, Error> {
        if !tokio::fs::metadata(dest).await?.is_dir() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotADirectory).into());
        }
        let collection = Collection::load(root.into(), &self.store)
            .await
            .map_err(Error::store)?;

        let mut targets = Vec::new();
        for (name, hash) in collection.iter() {
            let unsafe_name = |reason| Error::UnsafeName {
                name: name.clone(),
                reason,
            };
            safe_name::check(name).map_err(unsafe_name)?;
            // shortcut: exact-match only, so names differing in case clash on case-insensitive filesystems; upgrade when that bites.
            if targets.iter().any(|(n, _)| n == name) {
                return Err(unsafe_name(safe_name::NameError::Duplicate));
            }
            let path = dest.join(name);
            // shortcut: check-then-write is racy; fine for a single-user desktop app.
            if tokio::fs::try_exists(&path).await? {
                return Err(Error::AlreadyExists(path));
            }
            targets.push((name.clone(), (*hash, path)));
        }

        let mut written = Vec::new();
        for (_, (hash, path)) in targets {
            self.store
                .blobs()
                .export(hash, &path)
                .await
                .map_err(Error::store)?;
            written.push(path);
        }
        info!(share = %root.short(), files = written.len(), "share exported");
        Ok(written)
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

#[cfg(test)]
mod tests;
