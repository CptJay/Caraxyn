//! Caraxyn desktop app: owns one node and shows it in a Tauri window.

// No console window behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod dto;
mod error;
mod events;
mod settings;

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use caraxyn_core::node::{Node, NodeConfig};
use tauri::{Manager, RunEvent};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::{commands::AppState, error::UiError, settings::Settings};

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // ~/.local/share/caraxyn on Linux, %APPDATA%\caraxyn on Windows.
            // CARAXYN_DATA_DIR overrides it, to run a second instance for testing.
            let data_dir = match std::env::var_os("CARAXYN_DATA_DIR") {
                Some(dir) => {
                    let dir = PathBuf::from(dir);
                    // Tells test instances apart: "Caraxyn (a)", "Caraxyn (b)".
                    if let (Some(window), Some(name)) =
                        (app.get_webview_window("main"), dir.file_name())
                    {
                        window.set_title(&format!("Caraxyn ({})", name.to_string_lossy()))?;
                    }
                    dir
                }
                None => app.path().data_dir()?.join("caraxyn"),
            };
            std::fs::create_dir_all(&data_dir)?;
            app.manage(start_logging(&data_dir)?);

            let settings = Settings::load(&data_dir).unwrap_or_else(|e| {
                tracing::warn!(%e, "settings unreadable, using defaults");
                Settings::default()
            });
            let settings = Settings {
                download_dir: settings
                    .download_dir
                    .or_else(|| app.path().download_dir().ok()),
                ..settings
            };

            let config = NodeConfig::new(&data_dir);
            let (node, startup_error) = match tauri::async_runtime::block_on(Node::spawn(config)) {
                Ok(node) => {
                    tracing::info!(peer = %node.peer_id(), "node started");
                    events::forward(app.handle().clone(), &node);
                    (Some(node), None)
                }
                // Shown on the startup screen; the data folder is left as it is.
                Err(e) => {
                    tracing::error!(%e, "node failed to start");
                    (None, Some(UiError::from(e)))
                }
            };
            app.manage(AppState {
                node,
                startup_error,
                data_dir,
                settings: Mutex::new(settings),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::save_settings,
            commands::open_log_folder,
            commands::contacts,
            commands::create_invite,
            commands::redeem_invite,
            commands::remove_contact,
            commands::inbox,
            commands::accept,
            commands::decline,
            commands::forget_offer,
            commands::shares,
            commands::pending_offers,
            commands::share,
            commands::revoke,
            commands::set_expiry,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the app");

    app.run(|app, event| {
        if let RunEvent::Exit = event
            && let Some(node) = app.state::<AppState>().node.clone()
            && let Err(e) = tauri::async_runtime::block_on(node.shutdown())
        {
            tracing::warn!(%e, "node did not shut down cleanly");
        }
    });
}

/// How many daily log files to keep; older ones are deleted.
const LOG_DAYS: usize = 7;

/// Logs to stderr and to daily files in `data_dir/logs`, and logs panics.
/// The returned guard flushes the file on drop, so it lives as long as the app.
fn start_logging(data_dir: &Path) -> Result<WorkerGuard, Box<dyn std::error::Error>> {
    let logs = data_dir.join("logs");
    // The builder looks for old files to delete before it creates the folder.
    std::fs::create_dir_all(&logs)?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("caraxyn")
        .filename_suffix("log")
        .max_log_files(LOG_DAYS)
        .build(logs)?;
    let (file, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(fmt::layer())
        .with(fmt::layer().with_ansi(false).with_writer(file))
        .init();

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(%info, "panic");
        default_hook(info);
    }));
    Ok(guard)
}
