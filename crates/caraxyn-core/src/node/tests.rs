use std::time::Duration;

use tokio::sync::broadcast;

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
    node_with(lookup, TIMING).await
}

/// For tests that want to see garbage collection run.
const FAST_GC: Timing = Timing {
    gc_interval: Duration::from_millis(50),
    ..TIMING
};

/// Like [`offline_node`], with its own background timing.
async fn node_with(lookup: &MemoryLookup, timing: Timing) -> TestNode {
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
        timing,
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

    let root = a.node.share_file(&path, &[], None).await.unwrap();

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

    let first = a.node.share_file(&path, &[], None).await.unwrap();
    let second = a.node.share_file(&path, &[], None).await.unwrap();

    assert_eq!(first, second);
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn zero_byte_file_can_be_shared() {
    let a = offline_node(&MemoryLookup::new()).await;
    let (_src, path) = write_file("empty", b"");

    let root = a.node.share_file(&path, &[], None).await.unwrap();

    assert_eq!(
        read_share(&a.node, root).await,
        vec![("empty".to_owned(), Vec::new())]
    );
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_file_is_an_error() {
    let a = offline_node(&MemoryLookup::new()).await;

    let result = a
        .node
        .share_file(&a._dir.path().join("nope"), &[], None)
        .await;

    assert!(matches!(result, Err(Error::Io { .. })));
    a.node.shutdown().await.unwrap();
}

/// Large files are referenced in place, so editing one after import makes
/// reading it fail its hash check (small files are copied and unaffected).
#[tokio::test]
async fn editing_a_large_shared_file_breaks_it() {
    let a = offline_node(&MemoryLookup::new()).await;
    let (_src, path) = write_file("f.txt", &vec![1u8; 1 << 20]);
    let root = a.node.share_file(&path, &[], None).await.unwrap();

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
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();

    a.node.allow(b.node.peer_id()).unwrap();
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
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();

    a.node.allow(b.node.peer_id()).unwrap();
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
    a.node.allow(b.node.peer_id()).unwrap();

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
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
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
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();

    let refused = tokio::time::timeout(
        Duration::from_secs(20),
        b.node.fetch(a.node.peer_id(), root),
    )
    .await;
    println!("refusal surfaces as: {refused:?}");
    assert!(matches!(refused, Ok(Err(_))), "got {refused:?}");
    assert!(Collection::load(root.into(), &b.node.store).await.is_err());

    a.node.allow(b.node.peer_id()).unwrap();
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
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    a.node.allow(b.node.peer_id()).unwrap();
    a.node.disallow(b.node.peer_id()).unwrap();

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
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    a.node.allow(b.node.peer_id()).unwrap();
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
        .share_folder(&src.path().join("photos"), &[b.node.peer_id()], None)
        .await
        .unwrap();
    a.node.allow(b.node.peer_id()).unwrap();
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

    let h1 = a
        .node
        .share_folder(&one.path().join("d"), &[], None)
        .await
        .unwrap();
    let h2 = a
        .node
        .share_folder(&two.path().join("d"), &[], None)
        .await
        .unwrap();

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

    let root = a.node.share_folder(&d, &[], None).await.unwrap();

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
        .share_folder(&dir.path().join("empty"), &[], None)
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
        .share_folder(&src.path().join("photos"), &[], None)
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
        .share_folder(&src.path().join("photos"), &[], None)
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

// Unix only: Windows strips the trailing dot, so it cannot even create `notes.`.
#[cfg(unix)]
#[tokio::test]
async fn sharing_a_name_a_receiver_cannot_save_fails() {
    let a = offline_node(&MemoryLookup::new()).await;
    let dir = TempDir::new().unwrap();
    let d = dir.path().join("d");
    std::fs::create_dir(&d).unwrap();
    std::fs::write(d.join("ok.txt"), b"ok").unwrap();
    std::fs::write(d.join("notes."), b"x").unwrap();

    let result = a.node.share_folder(&d, &[], None).await;

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

    let result = a.node.share_file(&missing, &[], None).await;

    assert!(
        matches!(&result, Err(Error::Io { path, .. }) if *path == missing),
        "got {result:?}"
    );
    a.node.shutdown().await.unwrap();
}

/// Regression: when export moved files out of the store, deleting the saved
/// file made the content impossible to export or fetch again.
#[tokio::test]
async fn a_saved_file_can_be_deleted_and_received_again() {
    let content = vec![5u8; 3 << 20];
    let (a, b, root) = fetched(&content, "big.bin").await;
    let out = TempDir::new().unwrap();
    b.node.export(root, out.path()).await.unwrap();

    std::fs::remove_file(out.path().join("big.bin")).unwrap();

    b.node.fetch(a.node.peer_id(), root).await.unwrap();
    b.node.export(root, out.path()).await.unwrap();
    assert_eq!(std::fs::read(out.path().join("big.bin")).unwrap(), content);
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_peer_id_survives_a_restart() {
    let dir = TempDir::new().unwrap();
    let start = || Node::spawn_with(NodeConfig::new(dir.path()), presets::Minimal, None, TIMING);

    let first = start().await.unwrap();
    let id = first.peer_id();
    first.shutdown().await.unwrap();
    let second = start().await.unwrap();

    assert_eq!(second.peer_id(), id);
    second.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_second_node_cannot_use_the_same_data_dir() {
    let dir = TempDir::new().unwrap();
    let start = || Node::spawn_with(NodeConfig::new(dir.path()), presets::Minimal, None, TIMING);
    let first = start().await.unwrap();

    let second = start().await;

    assert!(matches!(second, Err(Error::Store(_))), "got {second:?}");
    first.shutdown().await.unwrap();
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

/// Waits until `done` holds, failing the test after 10 seconds.
async fn wait_for(what: &str, done: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !done() {
            tokio::time::sleep(FAST_GC.gc_interval).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting until {what}"));
}

#[tokio::test]
async fn a_received_share_is_kept_until_exported_then_freed() {
    let lookup = MemoryLookup::new();
    let a = offline_node(&lookup).await;
    let b = node_with(&lookup, FAST_GC).await;
    let content = vec![5u8; 3 << 20];
    let (_src, path) = write_file("big.bin", &content);
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    a.node.allow(b.node.peer_id()).unwrap();
    b.node.fetch(a.node.peer_id(), root).await.unwrap();
    let store_dir = b._dir.path().join("blobs");
    let fetched = disk_usage(&store_dir);

    // Several collection runs: the unexported share must survive them.
    tokio::time::sleep(FAST_GC.gc_interval * 6).await;
    let out = TempDir::new().unwrap();
    b.node.export(root, out.path()).await.unwrap();
    assert_eq!(std::fs::read(out.path().join("big.bin")).unwrap(), content);

    wait_for("the store copy is freed", || {
        disk_usage(&store_dir) + (3 << 20) <= fetched
    })
    .await;
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn garbage_collection_keeps_our_shares_and_never_touches_shared_files() {
    let lookup = MemoryLookup::new();
    let a = node_with(&lookup, FAST_GC).await;
    let b = offline_node(&lookup).await;
    // A big file is referenced in place, a small one is copied into the store.
    let src = TempDir::new().unwrap();
    let d = src.path().join("d");
    std::fs::create_dir(&d).unwrap();
    let big: Vec<u8> = (0..3 << 20).map(|i: u32| (i % 251) as u8).collect();
    std::fs::write(d.join("big.bin"), &big).unwrap();
    std::fs::write(d.join("small.txt"), b"small").unwrap();
    let root = a
        .node
        .share_folder(&d, &[b.node.peer_id()], None)
        .await
        .unwrap();

    tokio::time::sleep(FAST_GC.gc_interval * 6).await;

    assert_eq!(std::fs::read(d.join("big.bin")).unwrap(), big);
    a.node.allow(b.node.peer_id()).unwrap();
    b.node.fetch(a.node.peer_id(), root).await.unwrap();
    assert_eq!(
        read_share(&b.node, root).await,
        vec![
            ("d/big.bin".to_owned(), big),
            ("d/small.txt".to_owned(), b"small".to_vec())
        ]
    );
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn sharing_the_same_file_twice_leaves_one_tag() {
    let a = offline_node(&MemoryLookup::new()).await;
    let (_src, path) = write_file("a.txt", b"same");

    a.node.share_file(&path, &[], None).await.unwrap();
    a.node.share_file(&path, &[], None).await.unwrap();

    assert_eq!(a.node.store.tags().delete_all().await.unwrap(), 1);
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_unexported_share_survives_a_restart_and_collection() {
    let lookup = MemoryLookup::new();
    let a = offline_node(&lookup).await;
    let dir = TempDir::new().unwrap();
    let start = || {
        Node::spawn_with(
            NodeConfig::new(dir.path()),
            presets::Minimal,
            Some(lookup.clone()),
            FAST_GC,
        )
    };
    let b = start().await.unwrap();
    lookup.add_endpoint_info(b.router.endpoint().addr());
    let (_src, path) = write_file("big.bin", &vec![6u8; 1 << 20]);
    let root = a
        .node
        .share_file(&path, &[b.peer_id()], None)
        .await
        .unwrap();
    a.node.allow(b.peer_id()).unwrap();
    b.fetch(a.node.peer_id(), root).await.unwrap();
    b.shutdown().await.unwrap();
    // The sender goes away: the share can only come from b's own store now.
    a.node.shutdown().await.unwrap();

    let b = start().await.unwrap();
    tokio::time::sleep(FAST_GC.gc_interval * 6).await;
    let out = TempDir::new().unwrap();

    b.export(root, out.path()).await.unwrap();

    assert_eq!(
        std::fs::read(out.path().join("big.bin")).unwrap(),
        vec![6u8; 1 << 20]
    );
    b.shutdown().await.unwrap();
}

/// Starts a node on `dir` that the other test nodes can find through `lookup`.
async fn node_in(dir: &Path, lookup: &MemoryLookup) -> Node {
    let node = Node::spawn_with(
        NodeConfig::new(dir),
        presets::Minimal,
        Some(lookup.clone()),
        TIMING,
    )
    .await
    .unwrap();
    lookup.add_endpoint_info(node.router.endpoint().addr());
    node
}

#[tokio::test]
async fn the_allow_list_survives_a_restart() {
    let lookup = MemoryLookup::new();
    let dir = TempDir::new().unwrap();
    let (b, c) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let a = node_in(dir.path(), &lookup).await;
    let (_src, path) = write_file("hello.txt", b"hello");
    let root = a
        .share_file(&path, &[b.node.peer_id(), c.node.peer_id()], None)
        .await
        .unwrap();
    a.allow(b.node.peer_id()).unwrap();
    a.allow(c.node.peer_id()).unwrap();
    a.disallow(c.node.peer_id()).unwrap();
    a.shutdown().await.unwrap();

    let a = node_in(dir.path(), &lookup).await;

    b.node.fetch(a.peer_id(), root).await.unwrap();
    let refused =
        tokio::time::timeout(Duration::from_secs(20), c.node.fetch(a.peer_id(), root)).await;
    assert!(matches!(refused, Ok(Err(_))), "got {refused:?}");
    a.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
    c.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_fetch_that_gets_nothing_leaves_no_tag() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let (_src, path) = write_file("x.txt", b"x");
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();

    // Not allowed: the sender refuses, so nothing arrives.
    let refused = tokio::time::timeout(
        Duration::from_secs(20),
        b.node.fetch(a.node.peer_id(), root),
    )
    .await;

    assert!(matches!(refused, Ok(Err(_))), "got {refused:?}");
    assert_eq!(b.node.store.tags().delete_all().await.unwrap(), 0);
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_partly_received_share_keeps_its_tag() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    // A hand-made list the sender never shared (it lacks the file), so the fetch is
    // refused; what matters is that the receiver already holds the list.
    let list = Collection::from_iter([("x.txt".to_owned(), Hash::new(b"missing"))]);
    let root = list.clone().store(&a.node.store).await.unwrap();
    // The receiver already has the list, as after an interrupted download. Not left
    // to the network: when the sender gives up, QUIC may drop the list unread.
    let _list_on_b = list.store(&b.node.store).await.unwrap();
    a.node.allow(b.node.peer_id()).unwrap();

    let result = tokio::time::timeout(
        Duration::from_secs(20),
        b.node.fetch(a.node.peer_id(), root.hash().into()),
    )
    .await;

    assert!(matches!(result, Ok(Err(_))), "got {result:?}");
    assert_eq!(b.node.store.tags().delete_all().await.unwrap(), 1);
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_stranger_gets_a_control_answer_but_no_blobs() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let (_src, path) = write_file("hello.txt", b"hello");
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    let request = control::ControlMsg::ContactRequest {
        token: crate::contacts::InviteToken::random(),
        name: "B".into(),
    };

    let answer = b.node.request(a.node.peer_id(), &request).await.unwrap();

    assert_eq!(answer, control::ControlMsg::ContactRejected);
    let fetched = tokio::time::timeout(
        Duration::from_secs(20),
        b.node.fetch(a.node.peer_id(), root),
    )
    .await;
    assert!(matches!(fetched, Ok(Err(_))), "got {fetched:?}");
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

/// Sends raw bytes as one control message on `conn` and reads the answer.
async fn raw_request(
    conn: &iroh::endpoint::Connection,
    bytes: &[u8],
) -> Result<control::ControlMsg, control::ControlError> {
    let (mut tx, mut rx) = conn.open_bi().await.unwrap();
    // The receiver may stop reading part way (too large): that is a refusal too.
    if let Err(e) = tx.write_all(bytes).await {
        return Err(control::ControlError::Stream(e.to_string()));
    }
    tx.finish().unwrap();
    control::recv(&mut rx).await
}

#[tokio::test]
async fn bad_control_messages_fail_only_their_own_stream() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let conn = b
        .node
        .router
        .endpoint()
        .connect(a.node.router.endpoint().id(), control::CONTROL_ALPN)
        .await
        .unwrap();
    let valid = postcard::to_allocvec(&control::ControlMsg::ContactRequest {
        token: crate::contacts::InviteToken::random(),
        name: "B".into(),
    })
    .unwrap();

    let too_large = raw_request(&conn, &vec![0u8; 2 << 20]).await;
    let garbage = raw_request(&conn, b"\xff\xff garbage").await;
    // An answer sent as a request is refused too.
    let answer_as_request = raw_request(
        &conn,
        &postcard::to_allocvec(&control::ControlMsg::ContactRejected).unwrap(),
    )
    .await;
    let still_works = raw_request(&conn, &valid).await;

    for result in [too_large, garbage, answer_as_request] {
        assert!(result.is_err(), "got {result:?}");
    }
    assert_eq!(still_works.unwrap(), control::ControlMsg::ContactRejected);
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

/// Whether `from` can fetch `root` from `to` (a refusal is `false`).
async fn can_fetch(from: &Node, to: &Node, root: ContentHash) -> bool {
    let result =
        tokio::time::timeout(Duration::from_secs(20), from.fetch(to.peer_id(), root)).await;
    matches!(result, Ok(Ok(())))
}

#[tokio::test]
async fn an_invite_makes_two_devices_mutual_contacts() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let (_sa, pa) = write_file("from-a.txt", b"a");
    let (_sb, pb) = write_file("from-b.txt", b"b");
    let (root_a, root_b) = (
        a.node
            .share_file(&pa, &[b.node.peer_id()], None)
            .await
            .unwrap(),
        b.node
            .share_file(&pb, &[a.node.peer_id()], None)
            .await
            .unwrap(),
    );

    let code = a.node.create_invite("Alice").unwrap();
    let alice = b.node.redeem_invite(&code, "Bob").await.unwrap();

    assert_eq!(
        (alice.peer, alice.name.as_str()),
        (a.node.peer_id(), "Alice")
    );
    let a_contacts = a.node.contacts().unwrap();
    assert_eq!(a_contacts.len(), 1);
    assert_eq!(
        (a_contacts[0].peer, a_contacts[0].name.as_str()),
        (b.node.peer_id(), "Bob")
    );
    assert_eq!(b.node.contacts().unwrap(), vec![alice]);
    assert!(can_fetch(&b.node, &a.node, root_a).await);
    assert!(can_fetch(&a.node, &b.node, root_b).await);
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_invite_works_only_once() {
    let lookup = MemoryLookup::new();
    let (a, b, c) = (
        offline_node(&lookup).await,
        offline_node(&lookup).await,
        offline_node(&lookup).await,
    );
    let (_src, path) = write_file("x.txt", b"x");
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id(), c.node.peer_id()], None)
        .await
        .unwrap();
    let code = a.node.create_invite("Alice").unwrap();
    b.node.redeem_invite(&code, "Bob").await.unwrap();

    let second = c.node.redeem_invite(&code, "Carol").await;

    assert!(
        matches!(second, Err(Error::ContactRejected)),
        "got {second:?}"
    );
    assert!(c.node.contacts().unwrap().is_empty());
    assert_eq!(a.node.contacts().unwrap().len(), 1);
    assert!(!can_fetch(&c.node, &a.node, root).await);
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_forged_invite_is_rejected() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    // A's real id and a name, but a token A never issued.
    let forged = crate::contacts::encode_invite(&Invite {
        peer: a.node.peer_id(),
        name: "Alice".into(),
        token: InviteToken::random(),
    })
    .unwrap();

    let result = b.node.redeem_invite(&forged, "Bob").await;

    assert!(
        matches!(result, Err(Error::ContactRejected)),
        "got {result:?}"
    );
    assert!(a.node.contacts().unwrap().is_empty());
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_hostile_name_is_rejected_and_the_token_stays_unused() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let code = a.node.create_invite("Alice").unwrap();
    let token = crate::contacts::decode_invite(&code).unwrap().token;
    // Sent by hand: our own redeem_invite would refuse this name before sending.
    let hostile = control::ControlMsg::ContactRequest {
        token,
        name: "Bob\u{1b}[31m".into(),
    };

    let answer = b.node.request(a.node.peer_id(), &hostile).await.unwrap();

    assert_eq!(answer, control::ControlMsg::ContactRejected);
    assert!(a.node.contacts().unwrap().is_empty());
    // A bad request does not use up the invite.
    b.node.redeem_invite(&code, "Bob").await.unwrap();
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn contacts_survive_a_restart_and_removal_refuses_them() {
    let lookup = MemoryLookup::new();
    let dir = TempDir::new().unwrap();
    let a = node_in(dir.path(), &lookup).await;
    let b = offline_node(&lookup).await;
    let (_src, path) = write_file("x.txt", b"x");
    let root = a
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    let code = a.create_invite("Alice").unwrap();
    b.node.redeem_invite(&code, "Bob").await.unwrap();
    a.shutdown().await.unwrap();

    let a = node_in(dir.path(), &lookup).await;
    assert_eq!(a.contacts().unwrap().len(), 1);
    assert!(can_fetch(&b.node, &a, root).await);

    a.remove_contact(b.node.peer_id()).unwrap();

    assert!(a.contacts().unwrap().is_empty());
    let (_src2, path2) = write_file("y.txt", b"y");
    let root2 = a
        .share_file(&path2, &[b.node.peer_id()], None)
        .await
        .unwrap();
    assert!(!can_fetch(&b.node, &a, root2).await);
    a.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn bad_codes_and_names_fail_before_any_network() {
    let a = offline_node(&MemoryLookup::new()).await;

    let bad_code = a.node.redeem_invite("hello", "Bob").await;
    let bad_own_name = a.node.create_invite("");
    let code = a.node.create_invite("Alice").unwrap();
    let bad_redeem_name = a.node.redeem_invite(&code, "\u{7}").await;

    assert!(
        matches!(bad_code, Err(Error::InvalidInvite(_))),
        "got {bad_code:?}"
    );
    assert!(
        matches!(bad_own_name, Err(Error::InvalidInvite(_))),
        "got {bad_own_name:?}"
    );
    assert!(
        matches!(bad_redeem_name, Err(Error::InvalidInvite(_))),
        "got {bad_redeem_name:?}"
    );
    a.node.shutdown().await.unwrap();
}

/// A raw blobs connection from `from` to `to`, for sending hand-made requests.
async fn blob_conn(from: &Node, to: &Node) -> iroh::endpoint::Connection {
    from.router
        .endpoint()
        .connect(to.router.endpoint().id(), iroh_blobs::ALPN)
        .await
        .unwrap()
}

/// The hash of the single file inside a one-file share.
async fn file_hash(node: &Node, root: ContentHash) -> Hash {
    let collection = Collection::load(root.into(), &node.store).await.unwrap();
    let (_, hash) = collection.iter().next().unwrap();
    *hash
}

/// A shares `x.txt` with B only and `y.txt` with C only; all three are connected.
async fn two_shares() -> (TestNode, TestNode, TestNode, ContentHash, ContentHash) {
    let lookup = MemoryLookup::new();
    let (a, b, c) = (
        offline_node(&lookup).await,
        offline_node(&lookup).await,
        offline_node(&lookup).await,
    );
    a.node.allow(b.node.peer_id()).unwrap();
    a.node.allow(c.node.peer_id()).unwrap();
    let (_sx, px) = write_file("x.txt", b"for b");
    let (_sy, py) = write_file("y.txt", b"for c");
    let x = a
        .node
        .share_file(&px, &[b.node.peer_id()], None)
        .await
        .unwrap();
    let y = a
        .node
        .share_file(&py, &[c.node.peer_id()], None)
        .await
        .unwrap();
    (a, b, c, x, y)
}

#[tokio::test]
async fn each_recipient_gets_only_their_share() {
    let (a, b, c, x, y) = two_shares().await;

    assert!(can_fetch(&b.node, &a.node, x).await);
    assert!(!can_fetch(&b.node, &a.node, y).await);
    assert!(can_fetch(&c.node, &a.node, y).await);
    assert!(!can_fetch(&c.node, &a.node, x).await);
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_file_is_only_served_to_recipients_of_its_share() {
    let (a, b, c, x, y) = two_shares().await;
    let (file_x, file_y) = (file_hash(&a.node, x).await, file_hash(&a.node, y).await);
    let conn = blob_conn(&b.node, &a.node).await;
    let get = |hash| {
        let request = iroh_blobs::protocol::GetRequest::blob(hash);
        b.node.store.remote().execute_get(conn.clone(), request)
    };

    let own_file = get(file_x).await;
    let other_file = get(file_y).await;

    assert!(own_file.is_ok(), "got {own_file:?}");
    assert!(other_file.is_err(), "got {other_file:?}");
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

/// The attack the "lists only on a root" rule stops: a file shared with B whose
/// bytes happen to be the hash of C's file. Read as a list of hashes, it would
/// make A serve C's file to B.
#[tokio::test]
async fn a_file_is_never_read_as_a_list() {
    let (a, b, c, _, y) = two_shares().await;
    let file_y = file_hash(&a.node, y).await;
    let (_src, path) = write_file("innocent.bin", file_y.as_bytes());
    let x = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    let file_x = file_hash(&a.node, x).await;
    let conn = blob_conn(&b.node, &a.node).await;

    let as_list = b
        .node
        .store
        .remote()
        .execute_get(conn, iroh_blobs::protocol::GetRequest::all(file_x))
        .await;

    assert!(as_list.is_err(), "got {as_list:?}");
    assert!(
        !b.node.store.blobs().has(file_y).await.unwrap(),
        "C's file reached B"
    );
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn batch_requests_are_refused() {
    let (a, b, c, x, _) = two_shares().await;
    let file_x = file_hash(&a.node, x).await;
    let conn = blob_conn(&b.node, &a.node).await;

    let batch = b
        .node
        .store
        .remote()
        .execute_get_many(
            conn,
            iroh_blobs::protocol::GetManyRequest::from_iter([file_x]),
        )
        .await;

    assert!(batch.is_err(), "got {batch:?}");
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn asking_what_we_have_follows_the_same_rule() {
    use n0_future::StreamExt;
    let (a, b, c, x, y) = two_shares().await;
    let (file_x, file_y) = (file_hash(&a.node, x).await, file_hash(&a.node, y).await);
    let conn = blob_conn(&b.node, &a.node).await;
    let observe = |hash| {
        let request = iroh_blobs::protocol::ObserveRequest {
            hash,
            ranges: iroh_blobs::protocol::RangeSpec::all(),
        };
        b.node.store.remote().observe(conn.clone(), request)
    };

    let own = Box::pin(observe(file_x)).next().await;
    let other = Box::pin(observe(file_y)).next().await;

    assert!(matches!(own, Some(Ok(_))), "got {own:?}");
    assert!(!matches!(other, Some(Ok(_))), "got {other:?}");
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn sharing_again_adds_a_recipient() {
    let (a, b, c, x, _) = two_shares().await;
    assert!(!can_fetch(&c.node, &a.node, x).await);
    let collection = Collection::load(x.into(), &a.node.store).await.unwrap();
    assert_eq!(collection.len(), 1);
    let (_src, path) = write_file("x.txt", b"for b");

    let again = a
        .node
        .share_file(&path, &[c.node.peer_id()], None)
        .await
        .unwrap();

    assert_eq!(again, x);
    assert!(can_fetch(&c.node, &a.node, x).await);
    assert!(can_fetch(&b.node, &a.node, x).await);
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn shares_and_their_recipients_survive_a_restart() {
    let lookup = MemoryLookup::new();
    let dir = TempDir::new().unwrap();
    let (b, c) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let a = node_in(dir.path(), &lookup).await;
    a.allow(b.node.peer_id()).unwrap();
    a.allow(c.node.peer_id()).unwrap();
    let (_src, path) = write_file("x.txt", b"for b");
    let root = a
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    a.shutdown().await.unwrap();

    let a = node_in(dir.path(), &lookup).await;

    let shares = a.shares().unwrap();
    assert_eq!(shares.len(), 1);
    assert_eq!(shares[0].root, root);
    assert_eq!(shares[0].title, "x.txt");
    assert_eq!(shares[0].recipients, [b.node.peer_id()]);
    assert!(can_fetch(&b.node, &a, root).await);
    assert!(!can_fetch(&c.node, &a, root).await);
    a.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
    c.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn received_content_is_not_passed_on() {
    let (a, b, c, x, _) = two_shares().await;
    b.node.fetch(a.node.peer_id(), x).await.unwrap();
    // C may connect to B, but B never shared x with anyone.
    b.node.allow(c.node.peer_id()).unwrap();

    assert!(!can_fetch(&c.node, &b.node, x).await);
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_removed_peer_gets_nothing_new_on_a_connection_it_keeps_open() {
    let (a, b, c, x, _) = two_shares().await;
    let file_x = file_hash(&a.node, x).await;
    let conn = blob_conn(&b.node, &a.node).await;
    let get = |hash| {
        let request = iroh_blobs::protocol::GetRequest::blob(hash);
        b.node.store.remote().execute_get(conn.clone(), request)
    };
    assert!(get(file_x).await.is_ok());

    a.node.disallow(b.node.peer_id()).unwrap();

    // The list of x is not in B's store yet, so this is a real new request.
    let after = get(x.into()).await;
    assert!(after.is_err(), "got {after:?}");
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_removed_contact_invited_again_does_not_get_old_shares_back() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let first = a.node.create_invite("Alice").unwrap();
    b.node.redeem_invite(&first, "Bob").await.unwrap();
    let (_src, path) = write_file("old.txt", b"old");
    let old = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();

    a.node.remove_contact(b.node.peer_id()).unwrap();
    let again = a.node.create_invite("Alice").unwrap();
    b.node.redeem_invite(&again, "Bob").await.unwrap();

    assert!(!can_fetch(&b.node, &a.node, old).await);
    let (_src2, path2) = write_file("new.txt", b"new");
    let new = a
        .node
        .share_file(&path2, &[b.node.peer_id()], None)
        .await
        .unwrap();
    assert!(can_fetch(&b.node, &a.node, new).await);
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

/// Makes `a` and `b` contacts through an invite.
async fn befriend(a: &Node, b: &Node) {
    let code = a.create_invite("A").unwrap();
    b.redeem_invite(&code, "B").await.unwrap();
}

/// Offers go out in the background: waits until `node`'s inbox holds `count`.
async fn wait_for_inbox(node: &Node, count: usize) {
    wait_for("the offer arrives", || node.inbox().unwrap().len() == count).await;
}

/// Short retries, so tests see an offer arrive after its recipient comes back.
const FAST_RETRY: Timing = Timing {
    retry_first: Duration::from_millis(100),
    retry_max: Duration::from_millis(300),
    offer_timeout: Duration::from_secs(1),
    ..TIMING
};

#[tokio::test]
async fn an_offer_reaches_the_inbox_and_accepting_saves_the_share() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let src = photo_tree();

    let root = a
        .node
        .share_folder(&src.path().join("photos"), &[b.node.peer_id()], None)
        .await
        .unwrap();

    wait_for_inbox(&b.node, 1).await;
    let offer = &b.node.inbox().unwrap()[0];
    assert_eq!((offer.from, offer.root), (a.node.peer_id(), root));
    assert_eq!((offer.title.as_str(), offer.file_count), ("photos", 4));
    assert_eq!(offer.total_size, 1 + 1 + 1 + (1 << 20));
    assert_eq!(offer.preview.len(), 4);
    assert_eq!(offer.state, OfferState::New);
    wait_for("the outbox empties", || {
        a.node.pending_offers().unwrap().is_empty()
    })
    .await;

    let out = TempDir::new().unwrap();
    let written = b
        .node
        .accept(a.node.peer_id(), root, out.path())
        .await
        .unwrap();

    assert_eq!(written.len(), 4);
    assert!(out.path().join("photos/trip/deep/c.jpg").is_file());
    assert_eq!(b.node.inbox().unwrap()[0].state, OfferState::Done);
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_declined_offer_stays_declined_and_unknown_offers_are_errors() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let (_src, path) = write_file("x.txt", b"x");
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    wait_for_inbox(&b.node, 1).await;

    b.node.decline(a.node.peer_id(), root).unwrap();
    a.node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    wait_for("the second offer is delivered", || {
        a.node.pending_offers().unwrap().is_empty()
    })
    .await;

    assert_eq!(b.node.inbox().unwrap()[0].state, OfferState::Declined);
    let stranger = PeerId::from([5; 32]);
    let out = TempDir::new().unwrap();
    let accept = b.node.accept(stranger, root, out.path()).await;
    assert!(matches!(accept, Err(Error::UnknownOffer)), "got {accept:?}");
    assert!(matches!(
        b.node.decline(stranger, root),
        Err(Error::UnknownOffer)
    ));
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_forgotten_offer_frees_its_data_and_comes_back_as_new() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let (_src, path) = write_file("x.txt", b"x");
    let recipients = [b.node.peer_id()];
    let root = a.node.share_file(&path, &recipients, None).await.unwrap();
    wait_for_inbox(&b.node, 1).await;
    // Downloaded but never saved: its copy waits in the store under a tag.
    b.node.fetch(a.node.peer_id(), root).await.unwrap();
    b.node.decline(a.node.peer_id(), root).unwrap();

    b.node.forget_offer(a.node.peer_id(), root).await.unwrap();

    assert!(b.node.inbox().unwrap().is_empty());
    assert_eq!(
        b.node.store.tags().delete_all().await.unwrap(),
        0,
        "tag freed"
    );
    assert!(matches!(
        b.node.forget_offer(a.node.peer_id(), root).await,
        Err(Error::UnknownOffer)
    ));
    a.node.share_file(&path, &recipients, None).await.unwrap();
    wait_for_inbox(&b.node, 1).await;
    assert_eq!(b.node.inbox().unwrap()[0].state, OfferState::New);
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn offers_from_strangers_and_hostile_offers_are_refused() {
    let lookup = MemoryLookup::new();
    let (a, b, c) = (
        offline_node(&lookup).await,
        offline_node(&lookup).await,
        offline_node(&lookup).await,
    );
    befriend(&a.node, &b.node).await;
    let (_src, path) = write_file("x.txt", b"x");

    // C is not B's contact: B refuses, and C stops trying instead of retrying forever.
    c.node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    wait_for("the refused offer is dropped", || {
        c.node.pending_offers().unwrap().is_empty()
    })
    .await;
    // A is, but sends offers by hand that its own node would never make.
    let offer = |title: &str, preview: Vec<(String, u64)>| control::ControlMsg::ShareOffer {
        root: ContentHash::from([1; 32]),
        title: title.into(),
        file_count: 1,
        total_size: 1,
        preview,
        expires_at: None,
    };
    let bad_title = a
        .node
        .request(b.node.peer_id(), &offer("../evil", vec![]))
        .await;
    let bad_path = a
        .node
        .request(
            b.node.peer_id(),
            &offer("x", vec![("x/../../evil".into(), 1)]),
        )
        .await;
    let too_long = vec![("x/a".to_owned(), 1); PREVIEW_LEN + 1];
    let long_preview = a
        .node
        .request(b.node.peer_id(), &offer("x", too_long))
        .await;

    for result in [bad_title, bad_path, long_preview] {
        assert!(
            matches!(result, Err(control::ControlError::Refused)),
            "got {result:?}"
        );
    }
    assert!(b.node.inbox().unwrap().is_empty());
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_big_folder_is_offered_with_a_short_preview() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let src = TempDir::new().unwrap();
    let d = src.path().join("many");
    std::fs::create_dir(&d).unwrap();
    for i in 0..300 {
        std::fs::write(d.join(format!("{i:03}.txt")), b"x").unwrap();
    }

    a.node
        .share_folder(&d, &[b.node.peer_id()], None)
        .await
        .unwrap();

    wait_for_inbox(&b.node, 1).await;
    let offer = &b.node.inbox().unwrap()[0];
    assert_eq!((offer.file_count, offer.total_size), (300, 300));
    assert_eq!(offer.preview.len(), PREVIEW_LEN);
    assert_eq!(offer.preview[0], ("many/000.txt".to_owned(), 1));
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn sharing_returns_at_once_and_offline_recipients_wait_in_the_outbox() {
    let lookup = MemoryLookup::new();
    let (a, b, c) = (
        offline_node(&lookup).await,
        offline_node(&lookup).await,
        offline_node(&lookup).await,
    );
    befriend(&a.node, &b.node).await;
    befriend(&a.node, &c.node).await;
    let b_id = b.node.peer_id();
    b.node.shutdown().await.unwrap();
    let (_src, path) = write_file("x.txt", b"x");

    let started = std::time::Instant::now();
    let root = a
        .node
        .share_file(&path, &[b_id, c.node.peer_id()], None)
        .await
        .unwrap();

    assert!(
        started.elapsed() < Duration::from_secs(1),
        "took {:?}",
        started.elapsed()
    );
    wait_for_inbox(&c.node, 1).await;
    // C's answer arrives a moment before A records it as delivered.
    wait_for("C's offer leaves the outbox", || {
        a.node.pending_offers().unwrap().len() == 1
    })
    .await;
    let pending = a.node.pending_offers().unwrap();
    assert_eq!(
        (pending.len(), pending[0].to, pending[0].root),
        (1, b_id, root)
    );
    let out = TempDir::new().unwrap();
    c.node
        .accept(a.node.peer_id(), root, out.path())
        .await
        .unwrap();
    a.node.shutdown().await.unwrap();
    c.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_offline_recipient_gets_the_offer_once_back() {
    let lookup = MemoryLookup::new();
    let dir = TempDir::new().unwrap();
    let a = node_with(&lookup, FAST_RETRY).await;
    let b = node_in(dir.path(), &lookup).await;
    befriend(&a.node, &b).await;
    let b_id = b.peer_id();
    b.shutdown().await.unwrap();
    let (_src, path) = write_file("x.txt", b"x");
    let root = a.node.share_file(&path, &[b_id], None).await.unwrap();
    wait_for("a failed try", || {
        a.node
            .pending_offers()
            .unwrap()
            .iter()
            .any(|p| p.attempts > 0)
    })
    .await;

    // B comes back without contacting A: only the retry timer can deliver.
    let b = node_in(dir.path(), &lookup).await;

    wait_for_inbox(&b, 1).await;
    assert_eq!(b.inbox().unwrap()[0].root, root);
    wait_for("the outbox empties", || {
        a.node.pending_offers().unwrap().is_empty()
    })
    .await;
    a.node.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_recipient_that_connects_gets_its_offers_without_waiting() {
    let lookup = MemoryLookup::new();
    let dir = TempDir::new().unwrap();
    // Retries are far off (the real 30 s), so only the connection can trigger delivery.
    let quick_give_up = Timing {
        offer_timeout: Duration::from_secs(1),
        ..TIMING
    };
    let a = node_with(&lookup, quick_give_up).await;
    let b = node_in(dir.path(), &lookup).await;
    befriend(&a.node, &b).await;
    let b_id = b.peer_id();
    b.shutdown().await.unwrap();
    let (_src, path) = write_file("x.txt", b"x");
    a.node.share_file(&path, &[b_id], None).await.unwrap();
    wait_for("a failed try", || {
        a.node
            .pending_offers()
            .unwrap()
            .iter()
            .any(|p| p.attempts > 0)
    })
    .await;
    let b = node_in(dir.path(), &lookup).await;

    // Any control connection from B tells A that B is online.
    let started = std::time::Instant::now();
    b.request(a.node.peer_id(), &control::ControlMsg::ContactRejected)
        .await
        .ok();

    wait_for_inbox(&b, 1).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    a.node.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

#[tokio::test]
async fn queued_offers_go_out_when_the_sender_restarts() {
    let lookup = MemoryLookup::new();
    let (a_dir, b_dir) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    // Gives up on an offline peer fast, but would wait the real 30 s before retrying.
    let quick_give_up = Timing {
        offer_timeout: Duration::from_secs(1),
        ..TIMING
    };
    let start_a = || {
        Node::spawn_with(
            NodeConfig::new(a_dir.path()),
            presets::Minimal,
            Some(lookup.clone()),
            quick_give_up,
        )
    };
    let a = start_a().await.unwrap();
    lookup.add_endpoint_info(a.router.endpoint().addr());
    let b = node_in(b_dir.path(), &lookup).await;
    befriend(&a, &b).await;
    let b_id = b.peer_id();
    b.shutdown().await.unwrap();
    let (_src, path) = write_file("x.txt", b"x");
    a.share_file(&path, &[b_id], None).await.unwrap();
    wait_for("a failed try", || {
        a.pending_offers().unwrap().iter().any(|p| p.attempts > 0)
    })
    .await;
    a.shutdown().await.unwrap();

    let b = node_in(b_dir.path(), &lookup).await;
    let started = std::time::Instant::now();
    let a = start_a().await.unwrap();
    lookup.add_endpoint_info(a.router.endpoint().addr());

    // The next try was 30 s away, but a restart tries everything at once.
    wait_for_inbox(&b, 1).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    a.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_inbox_survives_a_restart() {
    let lookup = MemoryLookup::new();
    let dir = TempDir::new().unwrap();
    let a = offline_node(&lookup).await;
    let b = node_in(dir.path(), &lookup).await;
    befriend(&a.node, &b).await;
    let (_src, path) = write_file("x.txt", b"x");
    let root = a
        .node
        .share_file(&path, &[b.peer_id()], None)
        .await
        .unwrap();
    wait_for_inbox(&b, 1).await;
    b.shutdown().await.unwrap();

    let b = node_in(dir.path(), &lookup).await;

    let inbox = b.inbox().unwrap();
    assert_eq!((inbox.len(), inbox[0].root), (1, root));
    a.node.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_removed_contact_gets_none_of_the_offers_waiting_for_them() {
    let lookup = MemoryLookup::new();
    let dir = TempDir::new().unwrap();
    let a = node_with(&lookup, FAST_RETRY).await;
    let b = node_in(dir.path(), &lookup).await;
    befriend(&a.node, &b).await;
    let b_id = b.peer_id();
    b.shutdown().await.unwrap();
    let (_src, path) = write_file("secret-plans.txt", b"x");
    a.node.share_file(&path, &[b_id], None).await.unwrap();

    a.node.remove_contact(b_id).unwrap();

    assert!(a.node.pending_offers().unwrap().is_empty());
    let b = node_in(dir.path(), &lookup).await;
    // Several retry rounds pass (100-300 ms each): nothing may arrive.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(b.inbox().unwrap().is_empty());
    a.node.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

/// A stand-in peer that cancels every control request with `code`.
#[derive(Debug)]
struct Cancels(u32);

impl iroh::protocol::ProtocolHandler for Cancels {
    async fn accept(
        &self,
        conn: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        while let Ok((mut tx, mut rx)) = conn.accept_bi().await {
            rx.read_to_end(1 << 20).await.ok();
            tx.reset(self.0.into()).ok();
        }
        Ok(())
    }
}

/// Starts a [`Cancels`] peer reachable through `lookup`.
async fn cancelling_peer(lookup: &MemoryLookup, code: u32) -> (Router, PeerId) {
    let endpoint = Endpoint::builder(presets::Minimal)
        .address_lookup(lookup.clone())
        .bind()
        .await
        .unwrap();
    lookup.add_endpoint_info(endpoint.addr());
    let id = endpoint.id().into();
    let router = Router::builder(endpoint)
        .accept(control::CONTROL_ALPN, Cancels(code))
        .spawn();
    (router, id)
}

#[tokio::test]
async fn try_later_keeps_the_offer_and_a_refusal_drops_it() {
    let lookup = MemoryLookup::new();
    let a = node_with(&lookup, FAST_RETRY).await;
    // 1 = refused, 2 = try later (the receiver's own database failed).
    let (refuser, refused_id) = cancelling_peer(&lookup, 1).await;
    let (busy, busy_id) = cancelling_peer(&lookup, 2).await;
    let (_src, path) = write_file("x.txt", b"x");

    a.node
        .share_file(&path, &[refused_id, busy_id], None)
        .await
        .unwrap();

    wait_for("the busy peer was tried twice", || {
        a.node
            .pending_offers()
            .unwrap()
            .iter()
            .any(|p| p.to == busy_id && p.attempts >= 2)
    })
    .await;
    let waiting: Vec<_> = a
        .node
        .pending_offers()
        .unwrap()
        .iter()
        .map(|p| p.to)
        .collect();
    assert_eq!(waiting, vec![busy_id]);
    a.node.shutdown().await.unwrap();
    refuser.shutdown().await.unwrap();
    busy.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_revoked_share_is_refused_but_the_same_file_in_another_share_is_not() {
    let (a, b, c, x, _) = two_shares().await;
    // The same bytes as x's file, in a folder shared with C only.
    let src = TempDir::new().unwrap();
    std::fs::create_dir(src.path().join("d")).unwrap();
    std::fs::write(src.path().join("d/x.txt"), b"for b").unwrap();
    let folder = a
        .node
        .share_folder(&src.path().join("d"), &[c.node.peer_id()], None)
        .await
        .unwrap();
    let file_x = file_hash(&a.node, x).await;

    a.node.revoke(x).await.unwrap();

    assert!(!can_fetch(&b.node, &a.node, x).await);
    let conn = blob_conn(&b.node, &a.node).await;
    let direct = b
        .node
        .store
        .remote()
        .execute_get(conn, iroh_blobs::protocol::GetRequest::blob(file_x))
        .await;
    assert!(direct.is_err(), "got {direct:?}");
    assert!(can_fetch(&c.node, &a.node, folder).await);
    let unknown = a.node.revoke(ContentHash::from([4; 32])).await;
    assert!(
        matches!(unknown, Err(Error::UnknownShare)),
        "got {unknown:?}"
    );
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_recipient_is_told_and_a_saved_share_stays_saved() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let (_s1, p1) = write_file("kept.txt", b"kept");
    let (_s2, p2) = write_file("withdrawn.txt", b"gone");
    let kept = a
        .node
        .share_file(&p1, &[b.node.peer_id()], None)
        .await
        .unwrap();
    let gone = a
        .node
        .share_file(&p2, &[b.node.peer_id()], None)
        .await
        .unwrap();
    wait_for_inbox(&b.node, 2).await;
    let out = TempDir::new().unwrap();
    b.node
        .accept(a.node.peer_id(), kept, out.path())
        .await
        .unwrap();

    a.node.revoke(kept).await.unwrap();
    a.node.revoke(gone).await.unwrap();

    wait_for("both revokes are delivered", || {
        a.node.pending_offers().unwrap().is_empty()
    })
    .await;
    let state = |root| {
        b.node
            .inbox()
            .unwrap()
            .into_iter()
            .find(|o| o.root == root)
            .unwrap()
            .state
    };
    assert_eq!(state(kept), OfferState::Done);
    assert_eq!(state(gone), OfferState::Revoked);
    let accept = b.node.accept(a.node.peer_id(), gone, out.path()).await;
    assert!(matches!(accept, Err(Error::ShareRevoked)), "got {accept:?}");
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_offline_recipient_gets_the_revoke_instead_of_the_offer() {
    let lookup = MemoryLookup::new();
    let dir = TempDir::new().unwrap();
    let a = node_with(&lookup, FAST_RETRY).await;
    let b = node_in(dir.path(), &lookup).await;
    befriend(&a.node, &b).await;
    let b_id = b.peer_id();
    b.shutdown().await.unwrap();
    let (_src, path) = write_file("x.txt", b"x");
    let root = a.node.share_file(&path, &[b_id], None).await.unwrap();

    a.node.revoke(root).await.unwrap();

    let pending = a.node.pending_offers().unwrap();
    assert_eq!((pending.len(), pending[0].revoke), (1, true));
    let b = node_in(dir.path(), &lookup).await;
    // B never had the offer, so B refuses the revoke and A stops trying.
    wait_for("the revoke is settled", || {
        a.node.pending_offers().unwrap().is_empty()
    })
    .await;
    assert!(b.inbox().unwrap().is_empty());
    a.node.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_forgotten_share_is_gone_but_its_revoke_still_arrives() {
    let lookup = MemoryLookup::new();
    let dir = TempDir::new().unwrap();
    let a = node_with(&lookup, FAST_RETRY).await;
    let b = node_in(dir.path(), &lookup).await;
    befriend(&a.node, &b).await;
    let b_id = b.peer_id();
    let (_src, path) = write_file("x.txt", b"x");
    let root = a.node.share_file(&path, &[b_id], None).await.unwrap();
    wait_for_inbox(&b, 1).await;
    b.shutdown().await.unwrap();

    // Still active: forgetting it revokes it first.
    a.node.forget_share(root).await.unwrap();

    assert!(a.node.shares().unwrap().is_empty());
    assert!(matches!(
        a.node.forget_share(root).await,
        Err(Error::UnknownShare)
    ));
    let b = node_in(dir.path(), &lookup).await;
    assert!(!can_fetch(&b, &a.node, root).await);
    wait_for("the revoke arrives", || {
        a.node.pending_offers().unwrap().is_empty()
    })
    .await;
    assert_eq!(b.inbox().unwrap()[0].state, OfferState::Revoked);
    a.node.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

#[tokio::test]
async fn only_the_one_who_offered_can_revoke() {
    let lookup = MemoryLookup::new();
    let (a, b, c) = (
        offline_node(&lookup).await,
        offline_node(&lookup).await,
        offline_node(&lookup).await,
    );
    befriend(&a.node, &b.node).await;
    befriend(&c.node, &b.node).await;
    let (_src, path) = write_file("x.txt", b"x");
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    wait_for_inbox(&b.node, 1).await;

    let forged = c
        .node
        .request(b.node.peer_id(), &control::ControlMsg::Revoke { root })
        .await;

    assert!(
        matches!(forged, Err(control::ControlError::Refused)),
        "got {forged:?}"
    );
    assert_eq!(b.node.inbox().unwrap()[0].state, OfferState::New);
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn sharing_again_after_a_revoke_does_not_bring_back_the_old_recipients() {
    let (a, b, c, x, _) = two_shares().await;
    a.node.revoke(x).await.unwrap();
    let (_src, path) = write_file("x.txt", b"for b");

    let again = a
        .node
        .share_file(&path, &[c.node.peer_id()], None)
        .await
        .unwrap();

    assert_eq!(again, x);
    assert!(can_fetch(&c.node, &a.node, x).await);
    assert!(!can_fetch(&b.node, &a.node, x).await);
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn a_revoked_share_stays_revoked_after_a_restart() {
    let lookup = MemoryLookup::new();
    let dir = TempDir::new().unwrap();
    let b = offline_node(&lookup).await;
    let a = node_in(dir.path(), &lookup).await;
    a.allow(b.node.peer_id()).unwrap();
    let (_src, path) = write_file("x.txt", b"x");
    let root = a
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    a.revoke(root).await.unwrap();
    a.shutdown().await.unwrap();

    let a = node_in(dir.path(), &lookup).await;

    assert!(!can_fetch(&b.node, &a, root).await);
    a.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_expired_share_is_refused_until_its_expiry_moves() {
    let (a, b, c, _, _) = two_shares().await;
    let (_src, path) = write_file("late.txt", b"late");
    let past = crate::contacts::now_ms() - 1;
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], Some(past))
        .await
        .unwrap();
    assert!(!can_fetch(&b.node, &a.node, root).await);

    a.node.set_expiry(root, Some(past + 3_600_000)).unwrap();
    assert!(can_fetch(&b.node, &a.node, root).await);

    let unknown = a.node.set_expiry(ContentHash::from([4; 32]), None);
    assert!(
        matches!(unknown, Err(Error::UnknownShare)),
        "got {unknown:?}"
    );
    for n in [a, b, c] {
        n.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn an_offer_shows_its_expiry_and_the_sender_decides_when_it_is_up() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let (_src, path) = write_file("x.txt", b"x");
    let soon = crate::contacts::now_ms() + 1500;
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], Some(soon))
        .await
        .unwrap();
    wait_for_inbox(&b.node, 1).await;
    assert_eq!(b.node.inbox().unwrap()[0].expires_at, Some(soon));

    wait_for("the share expires", || crate::contacts::now_ms() >= soon).await;

    // Refused by the sender once expired...
    let out = TempDir::new().unwrap();
    let accept = b.node.accept(a.node.peer_id(), root, out.path()).await;
    assert!(matches!(accept, Err(Error::Store(_))), "got {accept:?}");
    // ...and accepted again once the sender moves the expiry, though B was not told.
    a.node.set_expiry(root, None).unwrap();
    b.node
        .accept(a.node.peer_id(), root, out.path())
        .await
        .unwrap();
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_already_expired_share_is_never_offered() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let (_src, path) = write_file("x.txt", b"x");
    let past = crate::contacts::now_ms() - 1;

    a.node
        .share_file(&path, &[b.node.peer_id()], Some(past))
        .await
        .unwrap();

    wait_for("the offer is dropped", || {
        a.node.pending_offers().unwrap().is_empty()
    })
    .await;
    assert!(b.node.inbox().unwrap().is_empty());
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn sharing_again_after_a_revoke_reaches_the_same_recipient() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let (_src, path) = write_file("x.txt", b"x");
    let x = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    wait_for_inbox(&b.node, 1).await;
    a.node.revoke(x).await.unwrap();
    wait_for("the revoke is delivered", || {
        b.node.inbox().unwrap()[0].state == OfferState::Revoked
    })
    .await;

    a.node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();

    wait_for("the new offer is delivered", || {
        b.node.inbox().unwrap()[0].state == OfferState::New
    })
    .await;
    let out = TempDir::new().unwrap();
    b.node
        .accept(a.node.peer_id(), x, out.path())
        .await
        .unwrap();
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn extending_an_expired_share_offers_it_again() {
    let lookup = MemoryLookup::new();
    let dir = TempDir::new().unwrap();
    let a = node_with(&lookup, FAST_RETRY).await;
    let b = node_in(dir.path(), &lookup).await;
    befriend(&a.node, &b).await;
    let b_id = b.peer_id();
    b.shutdown().await.unwrap();
    let (_src, path) = write_file("x.txt", b"x");
    let soon = crate::contacts::now_ms() + 500;
    let root = a.node.share_file(&path, &[b_id], Some(soon)).await.unwrap();
    // B stays away past the expiry, so A gives up on the offer.
    wait_for("the offer is dropped", || {
        crate::contacts::now_ms() > soon && a.node.pending_offers().unwrap().is_empty()
    })
    .await;

    a.node.set_expiry(root, None).unwrap();

    assert_eq!(a.node.pending_offers().unwrap().len(), 1);
    let b = node_in(dir.path(), &lookup).await;
    wait_for_inbox(&b, 1).await;
    let out = TempDir::new().unwrap();
    b.accept(a.node.peer_id(), root, out.path()).await.unwrap();
    a.node.shutdown().await.unwrap();
    b.shutdown().await.unwrap();
}

/// Waits up to 10 s for the first event `want` accepts, skipping others.
async fn next_event(
    events: &mut broadcast::Receiver<Event>,
    what: &str,
    want: impl Fn(&Event) -> bool,
) -> Event {
    let wait = async {
        loop {
            let event = events.recv().await.unwrap();
            if want(&event) {
                return event;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

#[tokio::test]
async fn both_sides_of_an_invite_hear_about_the_new_contact() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let (mut a_events, mut b_events) = (a.node.events(), b.node.events());

    befriend(&a.node, &b.node).await;

    let added = |e: &Event| matches!(e, Event::ContactAdded { .. });
    let Event::ContactAdded { contact: on_a } = next_event(&mut a_events, "A's event", added).await
    else {
        unreachable!()
    };
    let Event::ContactAdded { contact: on_b } = next_event(&mut b_events, "B's event", added).await
    else {
        unreachable!()
    };
    assert_eq!((on_a.peer, on_a.name.as_str()), (b.node.peer_id(), "B"));
    assert_eq!((on_b.peer, on_b.name.as_str()), (a.node.peer_id(), "A"));
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn offers_and_revokes_are_announced_on_both_sides() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let (mut a_events, mut b_events) = (a.node.events(), b.node.events());
    let (_src, path) = write_file("x.txt", b"x");

    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();

    let received = next_event(&mut b_events, "the offer", |e| {
        matches!(e, Event::OfferReceived { .. })
    })
    .await;
    assert!(matches!(received, Event::OfferReceived { offer } if offer.root == root));
    let delivered = Event::Delivered {
        to: b.node.peer_id(),
        root,
        revoke: false,
    };
    next_event(&mut a_events, "the delivery", |e| *e == delivered).await;

    a.node.revoke(root).await.unwrap();

    let revoked = Event::OfferRevoked {
        from: a.node.peer_id(),
        root,
    };
    next_event(&mut b_events, "the revoke", |e| *e == revoked).await;
    let revoke_delivered = Event::Delivered {
        to: b.node.peer_id(),
        root,
        revoke: true,
    };
    next_event(&mut a_events, "the revoke's delivery", |e| {
        *e == revoke_delivered
    })
    .await;
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_refused_offer_is_announced_as_undeliverable() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    let mut a_events = a.node.events();
    let (_src, path) = write_file("x.txt", b"x");

    // Not a contact: B refuses.
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();

    let undeliverable = Event::Undeliverable {
        to: b.node.peer_id(),
        root,
        revoke: false,
    };
    next_event(&mut a_events, "the refusal", |e| *e == undeliverable).await;
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[test]
fn a_connection_is_direct_when_any_address_in_use_is_not_a_relay() {
    assert_eq!(direct_from([]), None);
    assert_eq!(direct_from([true]), Some(false));
    assert_eq!(direct_from([true, false]), Some(true));
}

#[test]
fn a_download_needs_room_in_the_store_and_the_folder_twice_on_one_disk() {
    let (store, dest) = (Path::new("/store"), Path::new("/dest"));
    let disk = |id: &str, free| Disk {
        id: Some(id.into()),
        free,
    };

    // One disk: the store copy and the saved copy are there together for a while.
    assert!(enough_space(100, (store, &disk("a", 200)), (dest, &disk("a", 200))).is_ok());
    let tight = enough_space(100, (store, &disk("a", 150)), (dest, &disk("a", 150)));
    assert!(
        matches!(
            tight,
            Err(Error::NotEnoughSpace {
                needed: 200,
                available: 150,
                ..
            })
        ),
        "got {tight:?}"
    );

    // Two disks: each needs room for one copy, and the error names the full one.
    assert!(enough_space(100, (store, &disk("a", 100)), (dest, &disk("b", 100))).is_ok());
    let full = enough_space(100, (store, &disk("a", 500)), (dest, &disk("b", 99)));
    assert!(
        matches!(&full, Err(Error::NotEnoughSpace { path, needed: 100, .. }) if path == dest),
        "got {full:?}"
    );

    // Unknown disks count as different.
    let unknown = Disk {
        id: None,
        free: 100,
    };
    assert!(enough_space(100, (store, &unknown), (dest, &unknown)).is_ok());
}

#[tokio::test]
async fn the_sender_hears_how_a_download_of_its_share_goes() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let (_src, path) = write_file("big.bin", &vec![7u8; 3 << 20]);
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    wait_for_inbox(&b.node, 1).await;
    let mut a_events = a.node.events();

    b.node.fetch(a.node.peer_id(), root).await.unwrap();

    let done = next_event(&mut a_events, "the finished upload", |event| {
        matches!(event, Event::Upload { finished: true, .. })
    })
    .await;
    let Event::Upload {
        to,
        root: r,
        bytes,
        total,
        direct,
        ..
    } = done
    else {
        unreachable!()
    };
    assert_eq!((to, r, total), (b.node.peer_id(), root, 3 << 20));
    assert_eq!(direct, Some(true));
    // The file plus its small list of names.
    assert!(bytes >= 3 << 20, "sent {bytes}");
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn accepting_reports_progress_up_to_the_whole_share() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let content = vec![7u8; 3 << 20];
    let (_src, path) = write_file("big.bin", &content);
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    wait_for_inbox(&b.node, 1).await;
    let mut b_events = b.node.events();
    let out = TempDir::new().unwrap();

    b.node
        .accept(a.node.peer_id(), root, out.path())
        .await
        .unwrap();

    let mut seen = Vec::new();
    while let Ok(event) = b_events.try_recv() {
        if let Event::Progress {
            from,
            root: r,
            bytes,
            total,
            direct,
        } = event
        {
            assert_eq!((from, r, total), (a.node.peer_id(), root, Some(3 << 20)));
            // Two nodes on this machine reach each other without a relay.
            assert_eq!(direct, Some(true));
            seen.push(bytes);
        }
    }
    assert!(!seen.is_empty());
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "went down: {seen:?}");
    // The last one covers the whole share (the file plus its small list of names).
    assert!(*seen.last().unwrap() >= 3 << 20, "ended at {seen:?}");
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_listener_that_falls_behind_is_told_and_the_node_carries_on() {
    let a = offline_node(&MemoryLookup::new()).await;
    let mut slow = a.node.events();

    for i in 0..crate::events::EVENT_BUFFER + 10 {
        crate::events::warn_about(&a.node.events, "testing", i);
    }

    assert!(matches!(
        slow.recv().await,
        Err(broadcast::error::RecvError::Lagged(10))
    ));
    let (_src, path) = write_file("x.txt", b"x");
    a.node.share_file(&path, &[], None).await.unwrap();
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_download_with_nothing_left_to_fetch_still_ends_complete() {
    let lookup = MemoryLookup::new();
    let (a, b) = (offline_node(&lookup).await, offline_node(&lookup).await);
    befriend(&a.node, &b.node).await;
    let (_src, path) = write_file("big.bin", &vec![7u8; 1 << 20]);
    let root = a
        .node
        .share_file(&path, &[b.node.peer_id()], None)
        .await
        .unwrap();
    wait_for_inbox(&b.node, 1).await;
    // Already in B's store, so accepting downloads nothing.
    b.node.fetch(a.node.peer_id(), root).await.unwrap();
    let mut b_events = b.node.events();
    let out = TempDir::new().unwrap();

    b.node
        .accept(a.node.peer_id(), root, out.path())
        .await
        .unwrap();

    let mut last = None;
    while let Ok(event) = b_events.try_recv() {
        if let Event::Progress { bytes, total, .. } = event {
            last = Some((bytes, total));
        }
    }
    assert_eq!(last, Some((1 << 20, Some(1 << 20))));
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}

/// A stand-in sender that accepts blob requests and never answers them.
#[derive(Debug)]
struct Silent;

impl iroh::protocol::ProtocolHandler for Silent {
    async fn accept(
        &self,
        conn: iroh::endpoint::Connection,
    ) -> Result<(), iroh::protocol::AcceptError> {
        // Keeps every stream (and so the connection) open, sending nothing.
        let mut held = Vec::new();
        while let Ok(streams) = conn.accept_bi().await {
            held.push(streams);
        }
        Ok(())
    }
}

#[tokio::test]
async fn a_download_from_a_sender_that_goes_silent_gives_up() {
    let lookup = MemoryLookup::new();
    let quick_to_give_up = Timing {
        stall_limit: Duration::from_secs(1),
        ..TIMING
    };
    let b = node_with(&lookup, quick_to_give_up).await;
    let endpoint = Endpoint::builder(presets::Minimal)
        .address_lookup(lookup.clone())
        .bind()
        .await
        .unwrap();
    lookup.add_endpoint_info(endpoint.addr());
    let silent_id = endpoint.id().into();
    let silent = Router::builder(endpoint)
        .accept(iroh_blobs::ALPN, Silent)
        .spawn();

    let started = std::time::Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        b.node.fetch(silent_id, ContentHash::from([3; 32])),
    )
    .await;

    assert!(matches!(result, Ok(Err(Error::Stalled))), "got {result:?}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "took {:?}",
        started.elapsed()
    );
    b.node.shutdown().await.unwrap();
    silent.shutdown().await.unwrap();
}
