//! What the UI can ask for. Each command converts ids and calls the core; no logic lives here.
//!
//! Every command that touches the disk is `async`: Tauri runs plain `fn` commands on the
//! window's thread, so a slow database write would freeze the window.

use std::{path::PathBuf, sync::Mutex};

use caraxyn_core::{
    ids::{ContentHash, PeerId},
    node::Node,
};
use serde::Serialize;
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::{
    desktop,
    dto::{ContactDto, OfferDto, PendingDto, ShareDto, net_status_name},
    error::UiError,
    settings::Settings,
};

/// Shared by every command.
pub struct AppState {
    /// `None` when the node failed to start; see `startup_error`.
    pub node: Option<Node>,
    pub startup_error: Option<UiError>,
    pub data_dir: PathBuf,
    pub settings: Mutex<Settings>,
    /// Whether the tray icon could be added (not on Linux without AppIndicator).
    pub tray: bool,
}

impl AppState {
    fn node(&self) -> Result<&Node, UiError> {
        self.node
            .as_ref()
            .ok_or_else(|| UiError::new("NotRunning", "the node is not running"))
    }

    pub fn settings(&self) -> Settings {
        // A panic while holding the lock cannot leave settings half-written.
        self.settings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// The display name for invites; refuses until one is set.
    fn display_name(&self) -> Result<String, UiError> {
        let name = self.settings().display_name;
        if name.trim().is_empty() {
            return Err(UiError::new(
                "NoDisplayName",
                "set your display name in Settings first",
            ));
        }
        Ok(name)
    }
}

type Result<T, E = UiError> = std::result::Result<T, E>;

/// What the UI needs before showing anything.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    peer_id: Option<String>,
    /// `null` when the node failed to start.
    net_status: Option<&'static str>,
    startup_error: Option<String>,
    data_dir: PathBuf,
    settings: Settings,
    /// This app's version, from its Cargo.toml.
    version: &'static str,
    /// Whether there is a tray icon; without one, closing the window quits.
    tray: bool,
}

#[tauri::command]
pub fn status(state: State<'_, AppState>) -> Status {
    Status {
        peer_id: state.node.as_ref().map(|n| n.peer_id().to_string()),
        net_status: state.node.as_ref().map(|n| net_status_name(n.net_status())),
        startup_error: state.startup_error.as_ref().map(|e| e.message.clone()),
        data_dir: state.data_dir.clone(),
        settings: state.settings(),
        version: env!("CARGO_PKG_VERSION"),
        tray: state.tray,
    }
}

#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<()> {
    settings
        .save(&state.data_dir)
        .map_err(|e| UiError::new("Io", format!("could not save settings: {e}")))?;
    *state
        .settings
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = settings;
    Ok(())
}

#[tauri::command]
pub async fn open_log_folder(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    let logs = state.data_dir.join("logs");
    app.opener()
        .open_path(logs.to_string_lossy(), None::<&str>)
        .map_err(|e| UiError::new("Io", e.to_string()))
}

/// Stops the app, like Quit in the tray menu. Where no tray icon shows
/// (GNOME without AppIndicator), this is the only way out besides killing it.
#[tauri::command]
pub fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub async fn contacts(state: State<'_, AppState>) -> Result<Vec<ContactDto>> {
    Ok(state
        .node()?
        .contacts()?
        .into_iter()
        .map(Into::into)
        .collect())
}

#[tauri::command]
pub async fn create_invite(state: State<'_, AppState>) -> Result<String> {
    Ok(state.node()?.create_invite(&state.display_name()?)?)
}

#[tauri::command]
pub async fn redeem_invite(state: State<'_, AppState>, code: String) -> Result<ContactDto> {
    let name = state.display_name()?;
    Ok(state.node()?.redeem_invite(&code, &name).await?.into())
}

#[tauri::command]
pub async fn remove_contact(state: State<'_, AppState>, peer: String) -> Result<()> {
    Ok(state.node()?.remove_contact(peer.parse()?)?)
}

#[tauri::command]
pub async fn inbox(state: State<'_, AppState>) -> Result<Vec<OfferDto>> {
    Ok(state.node()?.inbox()?.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub async fn accept(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    from: String,
    root: String,
    dest: PathBuf,
) -> Result<Vec<PathBuf>> {
    let (from, root) = (from.parse()?, root.parse()?);
    let node = state.node()?;
    let written = node.accept(from, root, &dest).await?;
    // A long download may end while the window is in the background.
    let title = node
        .inbox()?
        .into_iter()
        .find(|offer| offer.from == from && offer.root == root)
        .map_or_else(|| "The share".to_owned(), |offer| offer.title);
    let folder = dest.file_name().map_or_else(
        || dest.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    desktop::notify(
        &app,
        "Download saved",
        &format!("{title} saved to {folder}"),
    );
    Ok(written)
}

#[tauri::command]
pub async fn decline(state: State<'_, AppState>, from: String, root: String) -> Result<()> {
    Ok(state.node()?.decline(from.parse()?, root.parse()?)?)
}

#[tauri::command]
pub async fn forget_offer(state: State<'_, AppState>, from: String, root: String) -> Result<()> {
    Ok(state
        .node()?
        .forget_offer(from.parse()?, root.parse()?)
        .await?)
}

#[tauri::command]
pub async fn shares(state: State<'_, AppState>) -> Result<Vec<ShareDto>> {
    Ok(state
        .node()?
        .shares()?
        .into_iter()
        .map(Into::into)
        .collect())
}

#[tauri::command]
pub async fn pending_offers(state: State<'_, AppState>) -> Result<Vec<PendingDto>> {
    Ok(state
        .node()?
        .pending_offers()?
        .into_iter()
        .map(Into::into)
        .collect())
}

/// Shares a file or a folder, whichever `path` is. Returns the share's hash.
#[tauri::command]
pub async fn share(
    state: State<'_, AppState>,
    path: PathBuf,
    recipients: Vec<String>,
    expires_at: Option<u64>,
) -> Result<String> {
    let recipients = recipients
        .iter()
        .map(|p| p.parse())
        .collect::<Result<Vec<PeerId>, _>>()?;
    let node = state.node()?;
    let is_dir = tokio::fs::metadata(&path)
        .await
        .map_err(|e| UiError::new("Io", format!("{}: {e}", path.display())))?
        .is_dir();
    let root = if is_dir {
        node.share_folder(&path, &recipients, expires_at).await?
    } else {
        node.share_file(&path, &recipients, expires_at).await?
    };
    Ok(root.to_string())
}

#[tauri::command]
pub async fn revoke(state: State<'_, AppState>, root: String) -> Result<()> {
    Ok(state.node()?.revoke(root.parse::<ContentHash>()?).await?)
}

#[tauri::command]
pub async fn forget_share(state: State<'_, AppState>, root: String) -> Result<()> {
    Ok(state.node()?.forget_share(root.parse()?).await?)
}

#[tauri::command]
pub async fn set_expiry(
    state: State<'_, AppState>,
    root: String,
    expires_at: Option<u64>,
) -> Result<()> {
    Ok(state.node()?.set_expiry(root.parse()?, expires_at)?)
}
