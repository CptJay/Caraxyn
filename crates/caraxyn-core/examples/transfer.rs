//! Sends a file or folder between two nodes over the real internet.
//!
//! Each side needs its own data folder; it holds that side's identity.
//!
//! ```text
//! cargo run --example transfer -- <data-dir> id
//! cargo run --example transfer -- <data-dir> invite <my-name>
//! cargo run --example transfer -- <data-dir> join <code> <my-name>
//! cargo run --example transfer -- <data-dir> share <path> [<allowed-peer-id>...]
//! cargo run --example transfer -- <data-dir> get <peer-id> <hash> <dest-folder>
//! ```
//!
//! Contacts (made with `invite` and `join`) may fetch every share, so `share`
//! needs no peer ids for them.
//!
//! More logs: `RUST_LOG=iroh=info,caraxyn_core=debug`.

use std::{error::Error, path::Path, process::ExitCode, time::Instant};

use caraxyn_core::{
    ids::{ContentHash, PeerId},
    node::{Node, NodeConfig},
};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: transfer <data-dir> id
       transfer <data-dir> invite <my-name>
       transfer <data-dir> join <code> <my-name>
       transfer <data-dir> share <path> [<allowed-peer-id>...]
       transfer <data-dir> get <peer-id> <hash> <dest-folder>";

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("caraxyn_core=info")),
        )
        // stderr, so the printed id and hash can be piped or captured on their own.
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    // Readable message instead of the debug form `main` would print for an error.
    match start(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn start(args: &[&str]) -> Result<(), Box<dyn Error>> {
    let [data_dir, command, rest @ ..] = args else {
        return Err(USAGE.into());
    };
    // Checked before spawning, so a typo does not create a data folder and identity.
    if !matches!(*command, "id" | "invite" | "join" | "share" | "get") {
        return Err(USAGE.into());
    }
    let node = Node::spawn(NodeConfig::new(data_dir)).await?;
    // Run the command, then always shut down cleanly, even when it failed.
    let result = run(&node, command, rest).await;
    node.shutdown().await?;
    result
}

async fn run(node: &Node, command: &str, rest: &[&str]) -> Result<(), Box<dyn Error>> {
    match (command, rest) {
        ("id", []) => println!("{}", node.peer_id()),
        ("invite", [my_name]) => {
            println!("send this one-time code to the other side (valid 24 hours):");
            println!("  {}", node.create_invite(my_name)?);
            println!("on the other side run:");
            println!("  cargo run --example transfer -- <data-dir> join <code> <their-name>");
            // Online until Enter: the other side's `join` has to reach this node.
            println!("press Enter to stop (after they joined)");
            wait_for_enter().await?;
        }
        ("join", [code, my_name]) => {
            let contact = node.redeem_invite(code, my_name).await?;
            println!("contact added: {} (peer {})", contact.name, contact.peer);
        }
        ("share", [path, allowed @ ..]) => {
            for peer in allowed {
                node.allow(parse::<PeerId>("allowed peer id", peer)?)?;
            }
            let path = Path::new(path);
            let hash = if path.is_dir() {
                node.share_folder(path).await?
            } else {
                node.share_file(path).await?
            };
            let contacts = node.contacts()?.len();
            println!(
                "sharing {} with {contacts} contact(s) and {} other peer(s)",
                path.display(),
                allowed.len()
            );
            println!("on the other side run:");
            println!(
                "  cargo run --example transfer -- <data-dir> get {} {hash} <dest-folder>",
                node.peer_id()
            );
            println!("press Enter to stop");
            wait_for_enter().await?;
        }
        ("get", [peer, hash, dest]) => {
            let peer = parse::<PeerId>("peer id", peer)?;
            let hash = parse::<ContentHash>("hash", hash)?;
            let start = Instant::now();
            node.fetch(peer, hash).await?;
            println!("fetched in {:.1?}", start.elapsed());
            for path in node.export(hash, Path::new(dest)).await? {
                println!("saved {}", path.display());
            }
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

/// Waits until Enter is pressed, or forever when stdin is closed (background,
/// no terminal), so the node keeps serving until it is killed.
async fn wait_for_enter() -> Result<(), Box<dyn Error>> {
    let read = tokio::task::spawn_blocking(|| std::io::stdin().read_line(&mut String::new()));
    if read.await?? == 0 {
        std::future::pending::<()>().await;
    }
    Ok(())
}

/// Parses one argument, naming it in the error so the user knows which one was wrong.
fn parse<T: std::str::FromStr<Err: std::fmt::Display>>(what: &str, s: &str) -> Result<T, String> {
    s.parse().map_err(|e| format!("{what}: {e}"))
}
