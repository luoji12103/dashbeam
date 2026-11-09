//! macOS file clipboard and actionable receive-completion notifications.

use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardLabels {
    pub copy_files: String,
    pub copy_paths: String,
    pub copy_failed: String,
    pub missing_files: String,
}

impl Default for ClipboardLabels {
    fn default() -> Self {
        Self {
            copy_files: "Copy files".into(),
            copy_paths: "Copy paths".into(),
            copy_failed: "Could not copy received files".into(),
            missing_files: "A received file was moved, removed, or is no longer readable.".into(),
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn validate_paths(paths: Vec<String>) -> Result<Vec<String>, String> {
    if paths.is_empty() || paths.len() > 100_000 {
        return Err("No received files are available, or too many files were selected".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::new();
    for path in paths {
        if path.contains('\0') || !std::path::Path::new(&path).is_absolute() {
            return Err("Received-file paths must be absolute filesystem paths".into());
        }
        if seen.insert(path.clone()) {
            unique.push(path);
        }
    }
    Ok(unique)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::{c_char, CStr, CString};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};
    use tauri::AppHandle;
    use tokio::sync::oneshot;

    type Reply = oneshot::Sender<Result<(), String>>;
    static PENDING: OnceLock<Mutex<HashMap<u64, Reply>>> = OnceLock::new();
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    static APP: Mutex<Option<AppHandle>> = Mutex::new(None);

    extern "C" {
        fn dashbeam_copy_received_files(json: *const c_char, as_paths: bool) -> i32;
        fn dashbeam_receive_notifications_available() -> bool;
        fn dashbeam_receive_notifications_init(open_callback: unsafe extern "C" fn());
        fn dashbeam_receive_notification_show(
            json: *const c_char,
            request_id: u64,
            callback: unsafe extern "C" fn(u64, *const c_char),
        );
    }

    unsafe extern "C" fn open_received_files() {
        let app = APP.lock().ok().and_then(|guard| guard.clone());
        if let Some(app) = app {
            crate::tray::open_and_focus(&app);
        }
    }

    unsafe extern "C" fn notification_scheduled(id: u64, error: *const c_char) {
        let reply = PENDING
            .get_or_init(Default::default)
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&id));
        if let Some(reply) = reply {
            let result = if error.is_null() {
                Ok(())
            } else {
                Err(CStr::from_ptr(error).to_string_lossy().into_owned())
            };
            let _ = reply.send(result);
        }
    }

    pub fn init(app: &AppHandle) {
        if let Ok(mut guard) = APP.lock() {
            *guard = Some(app.clone());
        }
        // Called by Tauri setup on its main thread, also before a response to
        // an older notification can arrive during a cold application launch.
        unsafe { dashbeam_receive_notifications_init(open_received_files) };
    }

    pub fn is_available() -> bool {
        unsafe { dashbeam_receive_notifications_available() }
    }

    pub async fn copy(app: &AppHandle, paths: Vec<String>, as_paths: bool) -> Result<(), String> {
        let paths = validate_paths(paths)?;
        let json = CString::new(serde_json::to_string(&paths).map_err(|e| e.to_string())?)
            .map_err(|_| "Invalid received-file path".to_string())?;
        let (tx, rx) = oneshot::channel();
        app.run_on_main_thread(move || {
            let result = unsafe { dashbeam_copy_received_files(json.as_ptr(), as_paths) };
            let _ = tx.send(match result {
                0 => Ok(()),
                1 => Err("A received file was moved, removed, or is no longer readable".into()),
                _ => Err("Could not write received files to the clipboard".into()),
            });
        })
        .map_err(|e| e.to_string())?;
        rx.await
            .map_err(|_| "Clipboard task was interrupted".to_string())?
    }

    pub async fn show(
        app: &AppHandle,
        title: String,
        body: Option<String>,
        paths: Vec<String>,
        labels: ClipboardLabels,
    ) -> Result<(), String> {
        let paths = validate_paths(paths)?;
        let json =
            serde_json::json!({ "title": title, "body": body, "paths": paths, "labels": labels });
        let json =
            CString::new(json.to_string()).map_err(|_| "Invalid notification".to_string())?;
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let pending = PENDING.get_or_init(Default::default);
        pending
            .lock()
            .map_err(|_| "Notification state unavailable")?
            .insert(id, tx);
        let scheduled = app.run_on_main_thread(move || unsafe {
            dashbeam_receive_notification_show(json.as_ptr(), id, notification_scheduled);
        });
        let result = match scheduled {
            Ok(()) => match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
                Ok(result) => {
                    result.map_err(|_| "Notification task was interrupted".to_string())?
                }
                Err(_) => Err("Notification authorization or delivery timed out".into()),
            },
            Err(error) => Err(error.to_string()),
        };
        // A delayed native callback is harmless once its pending sender is gone.
        if let Ok(mut guard) = pending.lock() {
            guard.remove(&id);
        }
        result
    }
}

#[cfg(target_os = "macos")]
pub use platform::{copy, init, is_available, show};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_actual_conflict_resolved_paths_and_selection_order() {
        let root = std::env::temp_dir().join("接收目录");
        let first = root.join("report (1).md").to_string_lossy().into_owned();
        let second = root
            .join("notes & draft.txt")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            validate_paths(vec![first.clone(), second.clone(), first.clone()]).unwrap(),
            vec![first, second]
        );
    }

    #[test]
    fn rejects_invalid_selections_instead_of_copying_download_directory() {
        for paths in [vec![], vec!["report.md".into()], vec!["/tmp/a\0b".into()]] {
            assert!(validate_paths(paths).is_err());
        }
    }
}
