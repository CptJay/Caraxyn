# Caraxyn

Peer-to-peer file sharing between people who know each other. Files go directly from your computer to your contact's: no account, no cloud, no upload limit.

[![CI](https://github.com/CptJay/Caraxyn/actions/workflows/ci.yml/badge.svg)](https://github.com/CptJay/Caraxyn/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/CptJay/Caraxyn)](https://github.com/CptJay/Caraxyn/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/CptJay/Caraxyn/total)](https://github.com/CptJay/Caraxyn/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
![Platforms](https://img.shields.io/badge/platform-Linux%20%7C%20Windows-lightgrey)

> **Status:** early release (0.1). Linux and Windows.

## Download

Get the latest version from the [Releases page](https://github.com/CptJay/Caraxyn/releases/latest).

- **Windows:** download `Caraxyn_…_x64-setup.exe` and run it. The installer is not code-signed yet, so Windows SmartScreen warns about an unknown publisher the first time: click **More info → Run anyway**.
- **Linux:** download `Caraxyn_…_amd64.AppImage`, make it executable (`chmod +x Caraxyn_*.AppImage`, or Properties → Allow executing) and run it. The tray icon needs AppIndicator (`libayatana-appindicator3`, included in Ubuntu); without it Caraxyn still runs, but closing the window quits it.

Caraxyn updates itself: new versions are offered in the app, and an update is only installed if its signature checks out.

## What it does

- **Contacts by invite code.** Create a code and send it to someone any way you like. When they paste it, you are connected. A code works once and expires after 24 hours. Only contacts can reach you; everyone else is refused.
- **Share files and folders** with one or more contacts. Files are shared from where they are, not copied or uploaded first.
- **Expiry and withdraw.** A share can expire after 1 hour, 24 hours or 7 days, or never. You can withdraw it at any time; from then on nobody can start downloading it.
- **Inbox.** Shares sent to you arrive as offers you accept (choosing where to save) or decline. Caraxyn checks there is enough free space first.
- **Live transfers.** Downloads and uploads show their speed, the time left, and whether the connection is direct or going through a relay.
- **Works while offline.** Offers wait in an outbox and are delivered when the contact comes online. An interrupted download continues where it stopped when you retry it.
- **Runs in the tray.** Close the window and contacts can keep downloading from you; notifications tell you about new shares and finished downloads.

Both sides need Caraxyn running for an offer to arrive and for a download to happen.

## How it works

- Built on [iroh](https://www.iroh.computer/): every device has a key pair, and its public key is its id. Connections are encrypted QUIC, direct when possible and through a relay otherwise.
- Content is sent with [iroh-blobs](https://github.com/n0-computer/iroh-blobs), so every byte is checked against its BLAKE3 hash.
- The sender checks every request: the content must belong to an active share the requester is a recipient of. Received content is never passed on to others.
- Received file names are checked before saving, so a name that is unsafe on Linux or Windows is refused, and nothing already on disk is overwritten.

## Privacy

- No account and no telemetry. Caraxyn does not report what you do to anyone.
- Files only ever go to the contacts you share them with, end-to-end encrypted.
- To find each other, devices use the public relay and address-lookup servers of [n0](https://n0.computer), iroh's makers. These can see your IP address and that your device is online, but never what you send. Anyone who knows your device id (it is in your invite codes) can look up how to reach it, but they cannot connect unless you added them as a contact.
- To check for updates, the app downloads a small file from GitHub when it starts and every 6 hours, so GitHub sees your IP address.

## Your data

Everything Caraxyn keeps is in one folder: `~/.local/share/caraxyn` on Linux, `%APPDATA%\caraxyn` on Windows (also shown in Settings). It holds your identity key (`secret.key`; keep it private), the database with your contacts and shares, the content store, `settings.json` and the logs (the last 7 days, in `logs/`). Back it up to keep your contacts on a new computer.

## Contributing

To build from source, run two instances on one machine, or send a change, see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Caraxyn is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
