//! Updates from GitHub Releases: the endpoint and the signing key are in `tauri.conf.json`.
//! An update is only installed when its signature matches that key.

use tauri::{AppHandle, State};
use tauri_plugin_updater::UpdaterExt;

use crate::{commands::AppState, error::UiError};

fn update_error(e: tauri_plugin_updater::Error) -> UiError {
    UiError::new("Update", format!("could not update: {e}"))
}

async fn find(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, UiError> {
    app.updater()
        .map_err(update_error)?
        .check()
        .await
        .map_err(update_error)
}

/// The newer version that is available, if any.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<String>, UiError> {
    Ok(find(&app).await?.map(|update| update.version))
}

/// Downloads and installs the newest version, then restarts into it.
#[tauri::command]
pub async fn install_update(app: AppHandle, state: State<'_, AppState>) -> Result<(), UiError> {
    let update = find(&app)
        .await?
        .ok_or_else(|| UiError::new("Update", "there is no newer version"))?;
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(update_error)?;
    // The Windows installer ends this process right away, before the usual exit
    // path runs, so the node is stopped here. Elsewhere the restart below stops it.
    #[cfg(windows)]
    if let Some(node) = state.node.clone() {
        node.shutdown().await?;
    }
    #[cfg(not(windows))]
    let _ = state;
    update.install(bytes).map_err(update_error)?;
    tracing::info!(version = %update.version, "update installed, restarting");
    app.restart()
}
