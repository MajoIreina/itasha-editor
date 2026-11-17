//! Application state, blocking command execution and desktop lifecycle.
use crate::platform::viewport;
use itasha_application::Application;
use itasha_contracts::{AppError, AppResult, Theme};
use itasha_persistence::Repository;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

pub(crate) type Service = Arc<Mutex<Application>>;

pub(crate) fn native_theme(theme: Theme) -> Option<tauri::Theme> {
    match theme {
        Theme::Dark => Some(tauri::Theme::Dark),
        Theme::Light => Some(tauri::Theme::Light),
        Theme::System => None,
    }
}

pub(crate) async fn blocking<T: Send + 'static>(
    service: Service,
    task: impl FnOnce(&mut Application) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut app = service
            .lock()
            .map_err(|_| AppError::new("internal-error", "工程服务异常，请重启应用。"))?;
        task(&mut app)
    })
    .await
    .map_err(AppError::io)?
}

pub(crate) fn builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            // Keep desktop acceptance isolated from user projects in release builds too.
            let root = std::env::var_os("ITASHA_TEST_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(root);
            let repository = Repository::new(root).map_err(|e| std::io::Error::other(e.message))?;
            let theme = repository
                .settings()
                .map_err(|e| std::io::Error::other(e.message))?
                .theme;
            if let Some(window) = app.get_webview_window("main") {
                window.set_theme(native_theme(theme))?;
            }
            app.manage(Arc::new(Mutex::new(Application::new(repository))));
            let window = app
                .get_webview_window("main")
                .ok_or("Main window missing")?;
            let renderer =
                viewport::NativeViewport::new(window.hwnd()?.0 as isize, app.handle().clone())
                    .map_err(std::io::Error::other)?;
            app.manage(renderer);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Always hand close to the UI; no blocking I/O on the window event loop.
                api.prevent_close();
                let _ = window.emit("close-requested", ());
            }
        })
}
