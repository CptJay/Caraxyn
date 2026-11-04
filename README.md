# Caraxyn

Peer-to-peer file sharing between people who know each other. Files go directly from your computer to your contact's: no account, no cloud, no upload limit.

> **Status:** early. The core works end to end; the desktop app is a first prototype. Linux and Windows.

## What it does

- **Contacts by invite code.** Create a code and send it to someone any way you like. When they paste it, you are connected. Only contacts can reach you; everyone else is refused.
- **Share files and folders** with one or more contacts. Files are shared from where they are, not copied first.
- **Expiry and withdraw.** A share can expire after 1 hour, 24 hours or 7 days, or never. You can withdraw it at any time; from then on nobody can start downloading it.
- **Inbox.** Shares sent to you arrive as offers you accept (choosing where to save) or decline.
- **Works while offline.** Offers wait in an outbox and are delivered when the contact comes online. An interrupted download continues where it stopped when you retry it.

## How it works

- Built on [iroh](https://www.iroh.computer/): every device has a key pair, and its public key is its id. Connections are encrypted QUIC, direct when possible and through a relay otherwise.
- Content is sent with [iroh-blobs](https://github.com/n0-computer/iroh-blobs), so every byte is checked against its BLAKE3 hash.
- The sender checks every request: the content must belong to an active share the requester is a recipient of. Received content is never passed on to others.
- Received file names are checked before saving, so a name that is unsafe on Linux or Windows is refused, and nothing already on disk is overwritten.

## Layout

```
crates/
  caraxyn-core/   the library: identity, contacts, shares, transfers, outbox (no UI)
  caraxyn/        the desktop app (Tauri v2)
    src/          Rust side: commands over the core, event forwarding, settings, logging
    ui/           frontend: Svelte 5 + TypeScript + Tailwind v4
```

## Building

You need:

- Rust (the version in `rust-toolchain.toml` is installed automatically by rustup)
- Node.js 22 and npm
- **Linux:** `sudo apt install libwebkit2gtk-4.1-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev` (or your distribution's equivalent)
- **Windows:** the Microsoft C++ build tools and WebView2 (already part of Windows 10/11)

Install the frontend packages once:

```bash
cd crates/caraxyn/ui && npm ci
```

Run the app in development mode (reloads on changes), from `crates/caraxyn`:

```bash
ui/node_modules/.bin/tauri dev
```

Build an installable release:

```bash
ui/node_modules/.bin/tauri build
```

### Two instances on one machine

To try invites and shares alone, give each instance its own data folder. With the first one running through `tauri dev`, start a second from the repository root:

```bash
CARAXYN_DATA_DIR=/tmp/caraxyn-b cargo run -p caraxyn
```

## Checks

The same checks CI runs:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

```bash
cd crates/caraxyn/ui && npm run check && npm run build
```

## Your data

Everything Caraxyn keeps is in one folder: `~/.local/share/caraxyn` on Linux, `%APPDATA%\caraxyn` on Windows. It holds your identity key (`secret.key`; keep it private), the database, the content store, `settings.json` and the logs (the last 7 days, in `logs/`).
