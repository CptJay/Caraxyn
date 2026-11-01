//! A running Caraxyn node.
//!
//! This is the only module that talks to iroh directly; everything else
//! sees Caraxyn's own types.

use iroh::{
    Endpoint, EndpointId,
    endpoint::presets::{self, Preset},
};
use tracing::info;

use crate::{error::Error, ids::PeerId};

impl From<EndpointId> for PeerId {
    fn from(id: EndpointId) -> Self {
        Self::from(*id.as_bytes())
    }
}

/// Handle to a running node. Cloning is cheap and shares the same node.
#[derive(Clone, Debug)]
pub struct Node {
    endpoint: Endpoint,
}

impl Node {
    /// Starts a node with a fresh identity, using iroh's default discovery
    /// and relay servers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Bind`] if the network endpoint cannot be started.
    pub async fn spawn() -> Result<Self, Error> {
        Self::spawn_with(presets::N0).await
    }

    async fn spawn_with(preset: impl Preset) -> Result<Self, Error> {
        let endpoint = Endpoint::bind(preset).await?;
        let node = Self { endpoint };
        info!(peer = %node.peer_id().short(), "node started");
        Ok(node)
    }

    /// This node's identity.
    #[must_use]
    pub fn peer_id(&self) -> PeerId {
        self.endpoint.id().into()
    }

    /// Stops the node and closes its connections.
    pub async fn shutdown(self) {
        self.endpoint.close().await;
        info!(peer = %self.peer_id().short(), "node stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests run offline: no discovery service and no relay.
    async fn offline_node() -> Node {
        Node::spawn_with(presets::Minimal).await.unwrap()
    }

    #[tokio::test]
    async fn peer_id_matches_the_endpoint_key() {
        let node = offline_node().await;
        let expected = *node.endpoint.id().as_bytes();
        assert_eq!(node.peer_id(), PeerId::from(expected));
        node.shutdown().await;
    }

    #[tokio::test]
    async fn two_nodes_have_different_ids() {
        let (a, b) = (offline_node().await, offline_node().await);
        assert_ne!(a.peer_id(), b.peer_id());
        a.shutdown().await;
        b.shutdown().await;
    }
}
