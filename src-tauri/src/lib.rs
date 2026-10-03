mod notice;

use std::path::PathBuf;

use robi::bootstrap::{self, AppConfig, Listen};
use tauri::{Manager, State};

/// The origin the webview should call, or empty when the Vite proxy is in use.
struct ApiOrigin(String);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    robi::logs::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let origin = if external_api() {
                tracing::info!("ROBI_EXTERNAL_API is set; not starting the in-process API");
                String::new()
            } else {
                let database_url = database_url(app)?;
                let settings_dir =
                    robi::adapters::settings::home_dir().map_err(|err| err.to_string())?;
                let listen = listen_from_env()?;
                let addr = bootstrap::spawn(
                    AppConfig {
                        database_url,
                        settings_dir,
                    },
                    listen,
                )
                .map_err(|err| err.to_string())?;
                let origin = format!("http://{addr}");
                tracing::info!(%origin, "in-process API listening");
                origin
            };
            app.manage(ApiOrigin(origin));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            notice::show_approval_notice,
            api_base_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// `http://127.0.0.1:<port>` for the in-process API, or `""` when the webview
/// should use the Vite `/api` proxy.
#[tauri::command]
fn api_base_url(origin: State<ApiOrigin>) -> String {
    origin.0.clone()
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
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("could not resolve the app data directory: {err}"))?;
    std::fs::create_dir_all(&dir).map_err(|err| {
        format!(
            "could not create the app data directory {}: {err}",
            dir.display()
        )
    })?;
    Ok(sqlite_url(&dir.join("robi.db")))
}

fn sqlite_url(path: &PathBuf) -> String {
    format!("sqlite://{}?mode=rwc", path.display())
}
