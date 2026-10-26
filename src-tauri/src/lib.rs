mod attachments;
mod notice;

#[cfg(target_os = "macos")]
mod mouse_nav;

use std::path::Path;

use robi::bootstrap::{self, AppConfig, Listen};
use tauri::{Manager, State};

/// The origin the webview should call, or empty when the Vite proxy is in use.
struct ApiOrigin(String);

/// The in-process server, kept so the app can stop it and remove the sandbox
/// containers before the process exits. Absent when `ROBI_EXTERNAL_API` is set.
struct InProcessApi(std::sync::Mutex<Option<bootstrap::ApiServer>>);

/// How long the exit may wait for the server and its cleanup.
///
/// A quit is a user action, so it stays snappy: a wedged daemon delays it by at
/// most this and leaves the container behind, which is what the crash gap says.
const EXIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    robi::logs::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let (origin, server) = match start_api(app) {
                Ok(started) => started,
                Err(err) => {
                    // Tauri turns this error into a panic on the AppKit thread,
                    // and that panic aborts. The line has to be on disk first.
                    tracing::error!(error = %err, "failed to start the in-process API");
                    robi::logs::record_error(&format!("failed to start the in-process API: {err}"));
                    return Err(err.into());
                }
            };
            app.manage(ApiOrigin(origin));
            app.manage(InProcessApi(std::sync::Mutex::new(server)));
            #[cfg(target_os = "macos")]
            mouse_nav::install(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            notice::show_approval_notice,
            attachments::pick_attachment_files,
            api_base_url
        ])
        .build(tauri::generate_context!())
        .expect("error while building the tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                stop_api(app);
            }
        });
}

/// `http://127.0.0.1:<port>` for the in-process API, or `""` when the webview
/// should use the Vite `/api` proxy.
#[tauri::command]
fn api_base_url(origin: State<ApiOrigin>) -> String {
    origin.0.clone()
}

fn start_api(app: &tauri::App) -> Result<(String, Option<bootstrap::ApiServer>), String> {
    if external_api() {
        tracing::info!("ROBI_EXTERNAL_API is set; not starting the in-process API");
        return Ok((String::new(), None));
    }
    let database_url = database_url(app)?;
    let settings_dir = robi::adapters::settings::home_dir().map_err(|err| err.to_string())?;
    let listen = listen_from_env()?;
    let server = bootstrap::spawn(
        AppConfig {
            database_url,
            settings_dir,
        },
        listen,
    )
    .map_err(|err| err.to_string())?;
    let origin = format!("http://{}", server.addr());
    tracing::info!(%origin, "in-process API listening");
    Ok((origin, Some(server)))
}

/// Stops the in-process server. Its thread removes the sandbox containers as
/// part of stopping, so the containers do not outlive the window.
///
/// Runs on the way out and removes the handle, so a second exit event is a no-op.
fn stop_api(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<InProcessApi>() else {
        return;
    };
    let taken = match state.0.lock() {
        Ok(mut slot) => slot.take(),
        Err(err) => {
            tracing::warn!(%err, "could not take the API server on exit");
            None
        }
    };
    if let Some(server) = taken {
        if !server.shutdown(EXIT_TIMEOUT) {
            tracing::warn!("exiting before the sandbox containers were removed");
        }
    }
}

/// Dev-only. Packaged builds always start the API in-process.
fn external_api() -> bool {
    tauri::is_dev() && std::env::var("ROBI_EXTERNAL_API").ok().as_deref() == Some("1")
}

fn listen_from_env() -> Result<Listen, String> {
    match std::env::var("ROBI_BIND") {
        Ok(raw) => bootstrap::parse_bind(&raw).map(Listen::Exact),
        Err(_) => Ok(Listen::Fallback {
            port: bootstrap::DEFAULT_PORT,
            attempts: bootstrap::PORT_ATTEMPTS,
        }),
    }
}

fn database_url(app: &tauri::App) -> Result<String, String> {
    if let Ok(url) = std::env::var("ROBI_DATABASE_URL") {
        return Ok(url);
    }
    let mut dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("could not resolve the app data directory: {err}"))?;
    // `tauri dev` and a packaged build share the identifier, so they would
    // otherwise share one file. A migration applied in dev then refuses to
    // open in an older installed binary.
    if tauri::is_dev() {
        dir.push("dev");
    }
    std::fs::create_dir_all(&dir).map_err(|err| {
        format!(
            "could not create the app data directory {}: {err}",
            dir.display()
        )
    })?;
    Ok(sqlite_url(&dir.join("robi.db")))
}

fn sqlite_url(path: &Path) -> String {
    format!("sqlite://{}?mode=rwc", path.display())
}
