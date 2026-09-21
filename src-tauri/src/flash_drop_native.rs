use tauri::AppHandle;

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::{c_char, CStr, CString};
    use std::sync::Mutex;

    use tauri::{AppHandle, Emitter as _};

    static APP_HANDLE: Mutex<Option<AppHandle>> = Mutex::new(None);

    type FilesCallback = unsafe extern "C" fn(*const *const c_char, usize);

    extern "C" {
        fn dashbeam_flash_drop_configure(
            target_label: *const c_char,
            available: bool,
            callback: FilesCallback,
        );
        fn dashbeam_flash_drop_stop();
        fn dashbeam_flash_drop_show_result(success: bool, message: *const c_char);
    }

    unsafe extern "C" fn receive_files(paths: *const *const c_char, count: usize) {
        if paths.is_null() || count == 0 {
            return;
        }

        let mut owned_paths = Vec::with_capacity(count);
        for index in 0..count {
            let path = *paths.add(index);
            if path.is_null() {
                continue;
            }
            owned_paths.push(CStr::from_ptr(path).to_string_lossy().into_owned());
        }

        if owned_paths.is_empty() {
            return;
        }
        if let Some(app) = APP_HANDLE.lock().ok().and_then(|guard| guard.clone()) {
            if let Err(error) = app.emit("flash-drop-files", owned_paths) {
                tracing::warn!(%error, "failed to emit flash-drop-files");
            }
        }
    }

    fn c_string(value: &str) -> CString {
        CString::new(value.replace('\0', " ")).expect("interior NUL bytes were removed")
    }

    pub fn configure(
        app: &AppHandle,
        enabled: bool,
        target_label: &str,
        available: bool,
    ) -> Result<(), String> {
        if !enabled {
            stop();
            return Ok(());
        }

        *APP_HANDLE
            .lock()
            .map_err(|_| "Flash Drop state lock was poisoned".to_string())? = Some(app.clone());
        let label = c_string(target_label);
        app.run_on_main_thread(move || unsafe {
            dashbeam_flash_drop_configure(label.as_ptr(), available, receive_files);
        })
        .map_err(|error| format!("Could not configure Flash Drop: {error}"))
    }

    pub fn stop() {
        if let Ok(mut guard) = APP_HANDLE.lock() {
            *guard = None;
        }
        unsafe { dashbeam_flash_drop_stop() };
    }

    pub fn show_result(success: bool, message: &str) {
        let message = c_string(message);
        unsafe { dashbeam_flash_drop_show_result(success, message.as_ptr()) };
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use tauri::AppHandle;

    pub fn configure(
        _app: &AppHandle,
        _enabled: bool,
        _target_label: &str,
        _available: bool,
    ) -> Result<(), String> {
        Ok(())
    }

    pub fn stop() {}

    pub fn show_result(_success: bool, _message: &str) {}
}

/// Starts or updates the native hot-corner drop target.
///
/// Passing `enabled = false` tears down every native event monitor. The native
/// implementation is otherwise event driven and does not run an idle timer.
pub fn configure(
    app: &AppHandle,
    enabled: bool,
    target_label: &str,
    available: bool,
) -> Result<(), String> {
    platform::configure(app, enabled, target_label, available)
}

/// Removes the native event monitors and drop panel.
pub fn stop() {
    platform::stop();
}

/// Keeps the panel visible briefly with the outcome of the requested send.
pub fn show_result(success: bool, message: &str) {
    platform::show_result(success, message);
}
