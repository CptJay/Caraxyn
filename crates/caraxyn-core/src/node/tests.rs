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
    node_with_gc(lookup, GC_INTERVAL).await
}

/// Garbage collection interval for tests that want to see it run.
const FAST_GC: Duration = Duration::from_millis(50);

/// Like [`offline_node`], with its own garbage collection interval.
async fn node_with_gc(lookup: &MemoryLookup, gc_interval: Duration) -> TestNode {
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
        gc_interval,
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

    let root = a.node.share_file(&path, &[]).await.unwrap();

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

    let first = a.node.share_file(&path, &[]).await.unwrap();
    let second = a.node.share_file(&path, &[]).await.unwrap();

    assert_eq!(first, second);
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn zero_byte_file_can_be_shared() {
    let a = offline_node(&MemoryLookup::new()).await;
    let (_src, path) = write_file("empty", b"");

    let root = a.node.share_file(&path, &[]).await.unwrap();

    assert_eq!(
        read_share(&a.node, root).await,
        vec![("empty".to_owned(), Vec::new())]
    );
    a.node.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_file_is_an_error() {
    let a = offline_node(&MemoryLookup::new()).await;

    let result = a.node.share_file(&a._dir.path().join("nope"), &[]).await;

    assert!(matches!(result, Err(Error::Io { .. })));
    a.node.shutdown().await.unwrap();
}

/// Large files are referenced in place, so editing one after import makes
/// reading it fail its hash check (small files are copied and unaffected).
#[tokio::test]
async fn editing_a_large_shared_file_breaks_it() {
    let a = offline_node(&MemoryLookup::new()).await;
    let (_src, path) = write_file("f.txt", &vec![1u8; 1 << 20]);
    let root = a.node.share_file(&path, &[]).await.unwrap();

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
    let root = a.node.share_file(&path, &[b.node.peer_id()]).await.unwrap();

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
    let root = a.node.share_file(&path, &[b.node.peer_id()]).await.unwrap();

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
    let root = a.node.share_file(&path, &[b.node.peer_id()]).await.unwrap();
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
    let root = a.node.share_file(&path, &[b.node.peer_id()]).await.unwrap();

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
    let root = a.node.share_file(&path, &[b.node.peer_id()]).await.unwrap();
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
    let root = a.node.share_file(&path, &[b.node.peer_id()]).await.unwrap();
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
        .share_folder(&src.path().join("photos"), &[b.node.peer_id()])
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
        .share_folder(&one.path().join("d"), &[])
        .await
        .unwrap();
    let h2 = a
        .node
        .share_folder(&two.path().join("d"), &[])
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

    let root = a.node.share_folder(&d, &[]).await.unwrap();

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
        .share_folder(&dir.path().join("empty"), &[])
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
        .share_folder(&src.path().join("photos"), &[])
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
        .share_folder(&src.path().join("photos"), &[])
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

    let result = a.node.share_folder(&d, &[]).await;

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

    let result = a.node.share_file(&missing, &[]).await;

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
    let start = || {
        Node::spawn_with(
            NodeConfig::new(dir.path()),
            presets::Minimal,
            None,
            GC_INTERVAL,
        )
    };

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
    let start = || {
        Node::spawn_with(
            NodeConfig::new(dir.path()),
            presets::Minimal,
            None,
            GC_INTERVAL,
        )
    };
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
            tokio::time::sleep(FAST_GC).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting until {what}"));
}

#[tokio::test]
async fn a_received_share_is_kept_until_exported_then_freed() {
    let lookup = MemoryLookup::new();
    let a = offline_node(&lookup).await;
    let b = node_with_gc(&lookup, FAST_GC).await;
    let content = vec![5u8; 3 << 20];
    let (_src, path) = write_file("big.bin", &content);
    let root = a.node.share_file(&path, &[b.node.peer_id()]).await.unwrap();
    a.node.allow(b.node.peer_id()).unwrap();
    b.node.fetch(a.node.peer_id(), root).await.unwrap();
    let store_dir = b._dir.path().join("blobs");
    let fetched = disk_usage(&store_dir);

    // Several collection runs: the unexported share must survive them.
    tokio::time::sleep(FAST_GC * 6).await;
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
    let a = node_with_gc(&lookup, FAST_GC).await;
    let b = offline_node(&lookup).await;
    // A big file is referenced in place, a small one is copied into the store.
    let src = TempDir::new().unwrap();
    let d = src.path().join("d");
    std::fs::create_dir(&d).unwrap();
    let big: Vec<u8> = (0..3 << 20).map(|i: u32| (i % 251) as u8).collect();
    std::fs::write(d.join("big.bin"), &big).unwrap();
    std::fs::write(d.join("small.txt"), b"small").unwrap();
    let root = a.node.share_folder(&d, &[b.node.peer_id()]).await.unwrap();

    tokio::time::sleep(FAST_GC * 6).await;

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

    a.node.share_file(&path, &[]).await.unwrap();
    a.node.share_file(&path, &[]).await.unwrap();

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
    let root = a.node.share_file(&path, &[b.peer_id()]).await.unwrap();
    a.node.allow(b.peer_id()).unwrap();
    b.fetch(a.node.peer_id(), root).await.unwrap();
    b.shutdown().await.unwrap();
    // The sender goes away: the share can only come from b's own store now.
    a.node.shutdown().await.unwrap();

    let b = start().await.unwrap();
    tokio::time::sleep(FAST_GC * 6).await;
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
        GC_INTERVAL,
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
        .share_file(&path, &[b.node.peer_id(), c.node.peer_id()])
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
    let root = a.node.share_file(&path, &[b.node.peer_id()]).await.unwrap();

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
    let root = a.node.share_file(&path, &[b.node.peer_id()]).await.unwrap();
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
        a.node.share_file(&pa, &[b.node.peer_id()]).await.unwrap(),
        b.node.share_file(&pb, &[a.node.peer_id()]).await.unwrap(),
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
        .share_file(&path, &[b.node.peer_id(), c.node.peer_id()])
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
    let root = a.share_file(&path, &[b.node.peer_id()]).await.unwrap();
    let code = a.create_invite("Alice").unwrap();
    b.node.redeem_invite(&code, "Bob").await.unwrap();
    a.shutdown().await.unwrap();

    let a = node_in(dir.path(), &lookup).await;
    assert_eq!(a.contacts().unwrap().len(), 1);
    assert!(can_fetch(&b.node, &a, root).await);

    a.remove_contact(b.node.peer_id()).unwrap();

    assert!(a.contacts().unwrap().is_empty());
    let (_src2, path2) = write_file("y.txt", b"y");
    let root2 = a.share_file(&path2, &[b.node.peer_id()]).await.unwrap();
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
    let x = a.node.share_file(&px, &[b.node.peer_id()]).await.unwrap();
    let y = a.node.share_file(&py, &[c.node.peer_id()]).await.unwrap();
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
    let x = a.node.share_file(&path, &[b.node.peer_id()]).await.unwrap();
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

    let again = a.node.share_file(&path, &[c.node.peer_id()]).await.unwrap();

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
    let root = a.share_file(&path, &[b.node.peer_id()]).await.unwrap();
    a.shutdown().await.unwrap();

    let a = node_in(dir.path(), &lookup).await;

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
    let old = a.node.share_file(&path, &[b.node.peer_id()]).await.unwrap();

    a.node.remove_contact(b.node.peer_id()).unwrap();
    let again = a.node.create_invite("Alice").unwrap();
    b.node.redeem_invite(&again, "Bob").await.unwrap();

    assert!(!can_fetch(&b.node, &a.node, old).await);
    let (_src2, path2) = write_file("new.txt", b"new");
    let new = a
        .node
        .share_file(&path2, &[b.node.peer_id()])
        .await
        .unwrap();
    assert!(can_fetch(&b.node, &a.node, new).await);
    a.node.shutdown().await.unwrap();
    b.node.shutdown().await.unwrap();
}
