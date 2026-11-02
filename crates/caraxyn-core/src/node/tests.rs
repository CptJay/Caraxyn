use std::time::Duration;

use tempfile::TempDir;

use super::*;

/// A node plus the temp dir that holds its data; the dir must outlive the node.
struct TestNode {
    node: Node,
    _dir: TempDir,
}

/// Starts an offline node (no discovery, no relay) that other test nodes can
/// find through `lookup`.
async fn offline_node(lookup: &MemoryLookup) -> TestNode {
    let dir = TempDir::new().unwrap();
    let node = Node::spawn_with(
        NodeConfig::new(dir.path()),
        presets::Minimal,
        Some(lookup.clone()),
    )
    .await
    .unwrap();
    lookup.add_endpoint_info(node.router.endpoint().addr());
    TestNode { node, _dir: dir }
}

/// Writes `content` to a file called `name` in a fresh temp dir.
fn write_file(name: &str, content: &[u8]) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, content).unwrap();
    (dir, path)
}

/// Reads back a share from a node's own store: its entries as `(name, bytes)`.
async fn read_share(node: &Node, root: ContentHash) -> Vec<(String, Vec<u8>)> {
    let collection = Collection::load(root.into(), &node.store).await.unwrap();
    let mut entries = Vec::new();
    for (name, hash) in collection.iter() {
        let bytes = node.store.blobs().get_bytes(*hash).await.unwrap();
        entries.push((name.clone(), bytes.to_vec()));
    }
    entries
}

#[tokio::test]
async fn peer_id_matches_the_endpoint_key() {
    let a = offline_node(&MemoryLookup::new()).await;
    let expected = *a.node.router.endpoint().id().as_bytes();
    assert_eq!(a.node.peer_id(), PeerId::from(expected));
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn two_nodes_have_different_ids() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    assert_ne!(a.node.peer_id(), b.node.peer_id());
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn shared_file_is_a_one_entry_collection() {
    let a = offline_node(&MemoryLookup::new()).await;
    let (_src, path) = write_file("hello.txt", b"hello world");

    let root = a.node.share_file(&path).await.unwrap();

    assert_eq!(
        read_share(&a.node, root).await,
        vec![("hello.txt".to_owned(), b"hello world".to_vec())]
    );
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn same_file_shared_twice_gives_same_hash() {
    let a = offline_node(&MemoryLookup::new()).await;
    let (_src, path) = write_file("a.bin", b"same bytes");

    let first = a.node.share_file(&path).await.unwrap();
    let second = a.node.share_file(&path).await.unwrap();

    assert_eq!(first, second);
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn zero_byte_file_can_be_shared() {
    let a = offline_node(&MemoryLookup::new()).await;
    let (_src, path) = write_file("empty", b"");

    let root = a.node.share_file(&path).await.unwrap();

    assert_eq!(
        read_share(&a.node, root).await,
        vec![("empty".to_owned(), Vec::new())]
    );
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_file_is_an_error() {
    let a = offline_node(&MemoryLookup::new()).await;

    let result = a.node.share_file(&a._dir.path().join("nope")).await;

    assert!(matches!(result, Err(Error::Io(_))));
    a.node.shutdown().await.unwrap();
}

/// Large files are referenced in place, so editing one after import makes
/// reading it fail its hash check (small files are copied and unaffected).
#[tokio::test]
async fn editing_a_large_shared_file_breaks_it() {
    let a = offline_node(&MemoryLookup::new()).await;
    let (_src, path) = write_file("f.txt", &vec![1u8; 1 << 20]);
    let root = a.node.share_file(&path).await.unwrap();

    std::fs::write(&path, vec![2u8; 1 << 20]).unwrap();

    let collection = Collection::load(root.into(), &a.node.store).await.unwrap();
    let (_, hash) = collection.iter().next().unwrap();
    assert!(a.node.store.blobs().get_bytes(*hash).await.is_err());
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn fetch_copies_a_small_file() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let (_src, path) = write_file("hello.txt", b"hello world");
    let root = a.node.share_file(&path).await.unwrap();

    a.node.allow(b.node.peer_id());
    b.node.fetch(a.node.peer_id(), root).await.unwrap();

    assert_eq!(
        read_share(&b.node, root).await,
        vec![("hello.txt".to_owned(), b"hello world".to_vec())]
    );
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn fetch_streams_a_large_file() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    // Not a repeating byte, so a corrupted or reordered chunk would show.
    let content: Vec<u8> = (0..3 << 20).map(|i: u32| (i % 251) as u8).collect();
    let (_src, path) = write_file("big.bin", &content);
    let root = a.node.share_file(&path).await.unwrap();

    a.node.allow(b.node.peer_id());
    b.node.fetch(a.node.peer_id(), root).await.unwrap();

    assert_eq!(
        read_share(&b.node, root).await,
        vec![("big.bin".to_owned(), content)]
    );
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn fetching_an_unknown_hash_fails() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    a.node.allow(b.node.peer_id());

    let result = tokio::time::timeout(
        Duration::from_secs(20),
        b.node.fetch(a.node.peer_id(), ContentHash::from([9; 32])),
    )
    .await;

    assert!(matches!(result, Ok(Err(_))), "got {result:?}");
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn fetching_from_a_stopped_node_fails() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let (_src, path) = write_file("hello.txt", b"hello world");
    let root = a.node.share_file(&path).await.unwrap();
    let a_id = a.node.peer_id();
    a.node.shutdown().await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(60), b.node.fetch(a_id, root)).await;

    assert!(matches!(result, Ok(Err(_))), "got {result:?}");
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_stranger_is_refused_until_allowed() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let (_src, path) = write_file("hello.txt", b"hello world");
    let root = a.node.share_file(&path).await.unwrap();

    let refused = tokio::time::timeout(
        Duration::from_secs(20),
        b.node.fetch(a.node.peer_id(), root),
    )
    .await;
    println!("refusal surfaces as: {refused:?}");
    assert!(matches!(refused, Ok(Err(_))), "got {refused:?}");
    assert!(Collection::load(root.into(), &b.node.store).await.is_err());

    a.node.allow(b.node.peer_id());
    b.node.fetch(a.node.peer_id(), root).await.unwrap();
    assert_eq!(read_share(&b.node, root).await.len(), 1);
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn disallowed_peer_is_refused_again() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let (_src, path) = write_file("hello.txt", b"hello world");
    let root = a.node.share_file(&path).await.unwrap();
    a.node.allow(b.node.peer_id());
    a.node.disallow(b.node.peer_id());

    let result = tokio::time::timeout(
        Duration::from_secs(20),
        b.node.fetch(a.node.peer_id(), root),
    )
    .await;

    assert!(matches!(result, Ok(Err(_))), "got {result:?}");
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

/// Stores a share holding one entry with the given (possibly hostile) name.
async fn share_with_name(node: &Node, name: &str, bytes: &'static [u8]) -> ContentHash {
    let file = node
        .store
        .blobs()
        .add_bytes(bytes)
        .temp_tag()
        .await
        .unwrap();
    let root = Collection::from_iter([(name.to_owned(), file.hash())])
        .store(&node.store)
        .await
        .unwrap();
    node.store
        .tags()
        .create(root.hash_and_format())
        .await
        .unwrap();
    root.hash().into()
}

/// A fetched share, exported on the receiving side.
async fn fetched(content: &[u8], name: &str) -> (TestNode, TestNode, ContentHash) {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let (_src, path) = write_file(name, content);
    let root = a.node.share_file(&path).await.unwrap();
    a.node.allow(b.node.peer_id());
    b.node.fetch(a.node.peer_id(), root).await.unwrap();
    (a, b, root)
}

#[tokio::test]
async fn export_writes_the_file() {
    let content: Vec<u8> = (0..3 << 20).map(|i: u32| (i % 251) as u8).collect();
    let (a, b, root) = fetched(&content, "big.bin").await;
    let out = TempDir::new().unwrap();

    let written = b.node.export(root, out.path()).await.unwrap();

    assert_eq!(written, vec![out.path().join("big.bin")]);
    assert_eq!(std::fs::read(&written[0]).unwrap(), content);
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn export_refuses_to_overwrite() {
    let (a, b, root) = fetched(b"new", "f.txt").await;
    let out = TempDir::new().unwrap();
    std::fs::write(out.path().join("f.txt"), b"old").unwrap();

    let result = b.node.export(root, out.path()).await;

    assert!(
        matches!(result, Err(Error::AlreadyExists(_))),
        "got {result:?}"
    );
    assert_eq!(std::fs::read(out.path().join("f.txt")).unwrap(), b"old");
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn export_of_an_unknown_share_fails() {
    let b = offline_node(&MemoryLookup::new()).await;
    let out = TempDir::new().unwrap();

    let result = b.node.export(ContentHash::from([9; 32]), out.path()).await;

    assert!(matches!(result, Err(Error::Store(_))), "got {result:?}");
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn export_into_a_missing_folder_fails() {
    let (a, b, root) = fetched(b"x", "f.txt").await;

    let result = b.node.export(root, &b._dir.path().join("nope")).await;

    assert!(matches!(result, Err(Error::Io(_))), "got {result:?}");
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn export_rejects_a_hostile_name_and_writes_nothing() {
    let node = offline_node(&MemoryLookup::new()).await;
    let root = share_with_name(&node.node, "../evil", b"x").await;
    let out = TempDir::new().unwrap();
    let inner = out.path().join("inner");
    std::fs::create_dir(&inner).unwrap();

    let result = node.node.export(root, &inner).await;

    assert!(
        matches!(&result, Err(Error::UnsafeName { name, .. }) if name == "../evil"),
        "got {result:?}"
    );
    assert!(!out.path().join("evil").exists());
    assert_eq!(std::fs::read_dir(&inner).unwrap().count(), 0);
    node.node.shutdown().await.unwrap();
}
