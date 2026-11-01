//! A running Caraxyn node.
//!
//! This is the only module that talks to iroh directly; everything else
//! sees Caraxyn's own types.

use std::path::{Path, PathBuf};

use iroh::{
    Endpoint, EndpointId,
    endpoint::presets::{self, Preset},
};
use iroh_blobs::{
    BlobFormat, Hash,
    api::{
        Store,
        blobs::{AddPathOptions, ImportMode},
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
    endpoint: Endpoint,
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
        Self::spawn_with(config, presets::N0).await
    }

    async fn spawn_with(config: NodeConfig, preset: impl Preset) -> Result<Self, Error> {
        let store = FsStore::load(config.data_dir.join("blobs"))
            .await
            .map_err(Error::store)?;
        let endpoint = Endpoint::bind(preset).await?;
        let node = Self {
            endpoint,
            store: store.into(),
        };
        info!(peer = %node.peer_id().short(), "node started");
        Ok(node)
    }

    /// This node's identity.
    #[must_use]
    pub fn peer_id(&self) -> PeerId {
        self.endpoint.id().into()
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

    /// Stops the node and closes its connections.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Store`] if the blob store does not shut down cleanly.
    pub async fn shutdown(self) -> Result<(), Error> {
        self.endpoint.close().await;
        self.store.shutdown().await.map_err(Error::store)?;
        info!(peer = %self.peer_id().short(), "node stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    /// Tests run offline: no discovery service and no relay.
    /// The `TempDir` must outlive the node, so it is returned too.
    async fn offline_node() -> (Node, TempDir) {
        let dir = TempDir::new().unwrap();
        let node = Node::spawn_with(NodeConfig::new(dir.path()), presets::Minimal)
            .await
            .unwrap();
        (node, dir)
    }

    /// Writes `content` to a file called `name` in a fresh temp dir.
    fn write_file(name: &str, content: &[u8]) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(name);
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    /// Reads back a share: its entries as `(name, bytes)`.
    async fn read_share(node: &Node, root: ContentHash) -> Vec<(String, Vec<u8>)> {
        let collection = Collection::load(Hash::from(*root.as_ref()), &node.store)
            .await
            .unwrap();
        let mut entries = Vec::new();
        for (name, hash) in collection.iter() {
            let bytes = node.store.blobs().get_bytes(*hash).await.unwrap();
            entries.push((name.clone(), bytes.to_vec()));
        }
        entries
    }

    #[tokio::test]
    async fn peer_id_matches_the_endpoint_key() {
        let (node, _dir) = offline_node().await;
        let expected = *node.endpoint.id().as_bytes();
        assert_eq!(node.peer_id(), PeerId::from(expected));
        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn two_nodes_have_different_ids() {
        let ((a, _da), (b, _db)) = (offline_node().await, offline_node().await);
        assert_ne!(a.peer_id(), b.peer_id());
        a.shutdown().await.unwrap();
        b.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shared_file_is_a_one_entry_collection() {
        let (node, _dir) = offline_node().await;
        let (_src, path) = write_file("hello.txt", b"hello world");

        let root = node.share_file(&path).await.unwrap();

        assert_eq!(
            read_share(&node, root).await,
            vec![("hello.txt".to_owned(), b"hello world".to_vec())]
        );
        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn same_file_shared_twice_gives_same_hash() {
        let (node, _dir) = offline_node().await;
        let (_src, path) = write_file("a.bin", b"same bytes");

        let first = node.share_file(&path).await.unwrap();
        let second = node.share_file(&path).await.unwrap();

        assert_eq!(first, second);
        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn zero_byte_file_can_be_shared() {
        let (node, _dir) = offline_node().await;
        let (_src, path) = write_file("empty", b"");

        let root = node.share_file(&path).await.unwrap();

        assert_eq!(
            read_share(&node, root).await,
            vec![("empty".to_owned(), Vec::new())]
        );
        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn missing_file_is_an_error() {
        let (node, dir) = offline_node().await;

        let result = node.share_file(&dir.path().join("nope")).await;

        assert!(matches!(result, Err(Error::Io(_))));
        node.shutdown().await.unwrap();
    }

    /// Large files are referenced in place, so editing one after import makes
    /// reading it fail its hash check (small files are copied and unaffected).
    #[tokio::test]
    async fn editing_a_large_shared_file_breaks_it() {
        let (node, _dir) = offline_node().await;
        let (_src, path) = write_file("f.txt", &vec![1u8; 1 << 20]);
        let root = node.share_file(&path).await.unwrap();

        std::fs::write(&path, vec![2u8; 1 << 20]).unwrap();

        let collection = Collection::load(Hash::from(*root.as_ref()), &node.store)
            .await
            .unwrap();
        let (_, hash) = collection.iter().next().unwrap();
        let result = node.store.blobs().get_bytes(*hash).await;
        assert!(result.is_err());
        node.shutdown().await.unwrap();
    }
}
