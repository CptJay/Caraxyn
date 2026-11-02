//! A running Caraxyn node.
//!
//! This is the only module that talks to iroh directly; everything else
//! sees Caraxyn's own types.

use std::path::{Path, PathBuf};

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
    store::fs::FsStore,
};
use tracing::info;

use crate::{
    error::Error,
    ids::{ContentHash, PeerId},
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
        let router = Router::builder(endpoint)
            .accept(iroh_blobs::ALPN, BlobsProtocol::new(&store, None))
            .spawn();
        let node = Self { router, store };
        info!(peer = %node.peer_id().short(), "node started");
        Ok(node)
    }

    /// This node's identity.
    #[must_use]
    pub fn peer_id(&self) -> PeerId {
        self.router.endpoint().id().into()
    }

    /// Downloads a share from another node into our own store.
    ///
    /// Everything is verified against its hash while it streams in. Returns
    /// once the whole collection is stored.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidPeerId`] if `from` is not a valid key, [`Error::Store`]
    /// if the peer is unreachable, does not have the share, or sends bad data.
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
