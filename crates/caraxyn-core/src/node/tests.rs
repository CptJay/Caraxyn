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
    // Shows logs with: RUST_LOG=caraxyn_core=debug cargo test -- --nocapture
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
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

    assert!(matches!(result, Err(Error::Io { .. })));
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

    assert!(matches!(result, Err(Error::Io { .. })), "got {result:?}");
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

/// Builds `photos/{a.jpg, .hidden, trip/b.jpg, trip/deep/c.jpg}` and returns the dir holding `photos`.
fn photo_tree() -> TempDir {
    let dir = TempDir::new().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir_all(photos.join("trip/deep")).unwrap();
    std::fs::write(photos.join("a.jpg"), b"a").unwrap();
    std::fs::write(photos.join(".hidden"), b"h").unwrap();
    std::fs::write(photos.join("trip/b.jpg"), b"b").unwrap();
    std::fs::write(photos.join("trip/deep/c.jpg"), vec![7u8; 1 << 20]).unwrap();
    dir
}

#[tokio::test]
async fn a_folder_round_trips() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let src = photo_tree();
    let root = a
        .node
        .share_folder(&src.path().join("photos"))
        .await
        .unwrap();
    a.node.allow(b.node.peer_id());
    b.node.fetch(a.node.peer_id(), root).await.unwrap();
    let out = TempDir::new().unwrap();

    let written = b.node.export(root, out.path()).await.unwrap();

    assert_eq!(written.len(), 4);
    let p = out.path().join("photos");
    assert_eq!(std::fs::read(p.join("a.jpg")).unwrap(), b"a");
    assert_eq!(std::fs::read(p.join(".hidden")).unwrap(), b"h");
    assert_eq!(std::fs::read(p.join("trip/b.jpg")).unwrap(), b"b");
    assert_eq!(
        std::fs::read(p.join("trip/deep/c.jpg")).unwrap().len(),
        1 << 20
    );
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn same_folder_gives_same_hash_whatever_the_creation_order() {
    let a = offline_node(&MemoryLookup::new()).await;
    let make = |order: [&str; 3]| {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("d")).unwrap();
        for n in order {
            std::fs::write(dir.path().join("d").join(n), n).unwrap();
        }
        dir
    };
    let (one, two) = (make(["x", "y", "z"]), make(["z", "y", "x"]));

    let h1 = a.node.share_folder(&one.path().join("d")).await.unwrap();
    let h2 = a.node.share_folder(&two.path().join("d")).await.unwrap();

    assert_eq!(h1, h2);
    a.node.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_are_not_shared() {
    let a = offline_node(&MemoryLookup::new()).await;
    let dir = TempDir::new().unwrap();
    let secret = dir.path().join("secret.txt");
    std::fs::write(&secret, b"secret").unwrap();
    let d = dir.path().join("d");
    std::fs::create_dir(&d).unwrap();
    std::fs::write(d.join("ok.txt"), b"ok").unwrap();
    std::os::unix::fs::symlink(&secret, d.join("link.txt")).unwrap();
    std::os::unix::fs::symlink(dir.path(), d.join("loop")).unwrap();

    let root = a.node.share_folder(&d).await.unwrap();

    let names: Vec<_> = read_share(&a.node, root)
        .await
        .into_iter()
        .map(|e| e.0)
        .collect();
    assert_eq!(names, vec!["d/ok.txt"]);
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_empty_folder_shares_nothing() {
    let a = offline_node(&MemoryLookup::new()).await;
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("empty")).unwrap();

    let root = a
        .node
        .share_folder(&dir.path().join("empty"))
        .await
        .unwrap();
    let out = TempDir::new().unwrap();

    assert!(a.node.export(root, out.path()).await.unwrap().is_empty());
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn hostile_folder_entries_are_rejected_and_write_nothing() {
    let node = offline_node(&MemoryLookup::new()).await;
    for name in ["a/../../evil", "/abs", "a//b", "a/CON", "a/"] {
        let root = share_with_name(&node.node, name, b"x").await;
        let out = TempDir::new().unwrap();

        let result = node.node.export(root, out.path()).await;

        assert!(
            matches!(result, Err(Error::UnsafeName { .. })),
            "{name}: {result:?}"
        );
        assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0, "{name}");
    }
    node.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn export_refuses_an_existing_top_level_folder() {
    let a = offline_node(&MemoryLookup::new()).await;
    let src = photo_tree();
    let root = a
        .node
        .share_folder(&src.path().join("photos"))
        .await
        .unwrap();
    let out = TempDir::new().unwrap();
    std::fs::create_dir(out.path().join("photos")).unwrap();

    let result = a.node.export(root, out.path()).await;

    assert!(
        matches!(result, Err(Error::AlreadyExists(_))),
        "got {result:?}"
    );
    assert_eq!(
        std::fs::read_dir(out.path().join("photos"))
            .unwrap()
            .count(),
        0
    );
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_file_and_a_folder_with_the_same_name_conflict() {
    let node = offline_node(&MemoryLookup::new()).await;
    let blob = node
        .node
        .store
        .blobs()
        .add_bytes("x")
        .temp_tag()
        .await
        .unwrap();
    for names in [["a", "a/b"], ["a/b", "a"], ["a", "a"]] {
        let root = Collection::from_iter(names.map(|n| (n.to_owned(), blob.hash())))
            .store(&node.node.store)
            .await
            .unwrap();
        let out = TempDir::new().unwrap();

        let result = node.node.export(root.hash().into(), out.path()).await;

        assert!(
            matches!(
                &result,
                Err(Error::UnsafeName {
                    reason: safe_name::NameError::Conflict,
                    ..
                })
            ),
            "{names:?}: {result:?}"
        );
    }
    node.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn export_accepts_a_relative_folder() {
    let a = offline_node(&MemoryLookup::new()).await;
    let src = photo_tree();
    let root = a
        .node
        .share_folder(&src.path().join("photos"))
        .await
        .unwrap();
    // A relative path to a temp dir, without changing the process-wide current dir.
    let out = TempDir::new_in(".").unwrap();
    let relative = Path::new(".").join(out.path().file_name().unwrap());

    let written = a.node.export(root, &relative).await.unwrap();

    assert_eq!(written.len(), 4);
    assert!(out.path().join("photos/trip/b.jpg").is_file());
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_failed_export_leaves_dest_as_it_was() {
    let node = offline_node(&MemoryLookup::new()).await;
    let present = node
        .node
        .store
        .blobs()
        .add_bytes("a")
        .temp_tag()
        .await
        .unwrap();
    // "d/a" is written first, then "d/b" fails: its data is not in the store.
    let missing = Hash::new(b"never added");
    let root = Collection::from_iter([
        ("d/a".to_owned(), present.hash()),
        ("d/b".to_owned(), missing),
    ])
    .store(&node.node.store)
    .await
    .unwrap();
    let out = TempDir::new().unwrap();

    let result = node.node.export(root.hash().into(), out.path()).await;

    assert!(matches!(result, Err(Error::Store(_))), "got {result:?}");
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
    node.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn sharing_a_name_a_receiver_cannot_save_fails() {
    let a = offline_node(&MemoryLookup::new()).await;
    let dir = TempDir::new().unwrap();
    let d = dir.path().join("d");
    std::fs::create_dir(&d).unwrap();
    std::fs::write(d.join("ok.txt"), b"ok").unwrap();
    std::fs::write(d.join("notes."), b"x").unwrap();

    let result = a.node.share_folder(&d).await;

    assert!(
        matches!(&result, Err(Error::UnsafeName { name, .. }) if name == "d/notes."),
        "got {result:?}"
    );
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn io_errors_name_the_path() {
    let a = offline_node(&MemoryLookup::new()).await;
    let missing = a._dir.path().join("nope");

    let result = a.node.share_file(&missing).await;

    assert!(
        matches!(&result, Err(Error::Io { path, .. }) if *path == missing),
        "got {result:?}"
    );
    a.node.shutdown().await.unwrap();
}

/// Total size of all files under `dir`.
fn disk_usage(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .map(|e| match e.file_type().unwrap().is_dir() {
            true => disk_usage(&e.path()),
            false => e.metadata().unwrap().len(),
        })
        .sum()
}

#[tokio::test]
async fn export_moves_a_received_file_out_of_the_store() {
    let content = vec![5u8; 3 << 20];
    let (a, b, root) = fetched(&content, "big.bin").await;
    let store_dir = b._dir.path().join("blobs");
    let before = disk_usage(&store_dir);
    let out = TempDir::new().unwrap();

    b.node.export(root, out.path()).await.unwrap();

    assert_eq!(std::fs::read(out.path().join("big.bin")).unwrap(), content);
    // The store keeps its database and small metadata files, not the data.
    assert!(
        disk_usage(&store_dir) + (3 << 20) <= before,
        "store still holds a copy"
    );
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}
