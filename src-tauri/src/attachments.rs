//! Pick files for a chat attachment, through the OS dialog.
//!
//! The webview's `<input type="file">` hands the page a `File` with only its
//! basename — no directory — so the server cannot tell whether an attachment is
//! inside the workspace. The native dialog returns a real absolute path, and
//! this command reads the bytes and returns them base64-encoded alongside it.
//! The server then decides in-workspace vs outside from the path.

use std::fs;

use base64::Engine;
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

/// One picked file: its name, its absolute path, and its bytes.
#[derive(Serialize)]
pub struct PickedAttachment {
    /// The file's base name, for display.
    pub name: String,
    /// The absolute path, for the server to classify. Never stored.
    #[serde(rename = "absolutePath")]
    pub absolute_path: String,
    /// The file's bytes, standard base64.
    #[serde(rename = "contentBase64")]
    pub content_base64: String,
}

/// Open the OS file picker and read the chosen files. An empty list when the
/// user cancels.
///
/// `async` on purpose: Tauri runs a sync command on the main thread, and
/// `blocking_pick_files` waits for a dialog the main thread has to service — on
/// the main thread that deadlocks and nothing opens. An async command runs on a
/// worker, so the blocking picker is safe. A file that cannot be read is
/// skipped; the caller sees a shorter list.
#[tauri::command]
pub async fn pick_attachment_files(app: AppHandle) -> Result<Vec<PickedAttachment>, String> {
    let picked = app
        .dialog()
        .file()
        .set_title("Attach files")
        .blocking_pick_files();
    let Some(paths) = picked else {
        return Ok(Vec::new());
    };

    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let path = path.into_path().map_err(|err| err.to_string())?;
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(PickedAttachment {
            name,
            absolute_path: path.to_string_lossy().into_owned(),
            content_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        });
    }
    Ok(out)
}
