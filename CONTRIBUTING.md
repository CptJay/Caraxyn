# Contributing

Bug reports and ideas are welcome in [Issues](https://github.com/CptJay/Caraxyn/issues). This page is about building Caraxyn from source and working on it.

## Layout

```
crates/
  caraxyn-core/   the library: identity, contacts, shares, transfers, outbox (no UI)
  caraxyn/        the desktop app (Tauri v2)
    src/          Rust side: commands over the core, event forwarding, settings, updates, logging
    ui/           frontend: Svelte 5 + TypeScript + Tailwind v4
scripts/          two local instances for testing
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

Build an installer (an AppImage on Linux, an NSIS `.exe` on Windows, in `target/release/bundle/`). Release builds are signed for the auto-updater; without the signing key, turn that off:

```bash
ui/node_modules/.bin/tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

The AppImage carries its own GLib and, to match it, the tray libraries from the build machine (listed under `bundle.linux.appimage.files` in `tauri.conf.json`). Using the system's tray library instead fails on newer distributions, whose copy needs a newer GLib. Build AppImages on the oldest system you support (CI uses Ubuntu 22.04).

## Two instances on one machine

To try invites and shares alone, run two instances, "a" and "b", each with its own data folder (kept in `target/two-instances/`; delete it to start fresh). The windows are titled `Caraxyn (a)` and `Caraxyn (b)`. Ctrl-C stops both.

Linux:

```bash
scripts/two-instances.sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\two-instances.ps1
```

Under the hood, `CARAXYN_DATA_DIR` sets an instance's data folder.

## Checks

The same checks CI runs on every pull request:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

```bash
cd crates/caraxyn/ui && npm run check && npm test && npm run build
```

## Rules worth knowing

- Contact names, share titles and file names come from other people: the UI shows them as text only, never as HTML (`{@html}` is a lint error).
- The database has a format number. Changing a stored type means bumping `FORMAT` in `crates/caraxyn-core/src/node/db.rs` with an upgrade step; a test fails if the saved bytes change without it.

## License

Unless you explicitly state otherwise, any contribution you submit for inclusion in Caraxyn, as defined in the Apache-2.0 license, is dual licensed under MIT OR Apache-2.0, without any additional terms or conditions.
