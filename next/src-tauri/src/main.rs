#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use itasha_application::{Action, Application, View};
use itasha_contracts::{AppError, AppResult, MutationRequest, SessionRef, Theme};
use itasha_persistence::Repository;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use uuid::Uuid;
mod model_package;
mod viewport;

#[tauri::command]
async fn import_image_path(
    state: tauri::State<'_, Service>,
    renderer: tauri::State<'_, viewport::NativeViewport>,
    mutation: MutationRequest,
    path: String,
) -> AppResult<itasha_contracts::ImageAsset> {
    let result = blocking(state.inner().clone(), move |core| {
        core.image_import_check(&mutation)?;
        let path = std::path::PathBuf::from(path);
        let file = std::fs::File::open(&path).map_err(AppError::io)?;
        if !file.metadata().map_err(AppError::io)?.is_file() {
            return Err(AppError::new("image-invalid", "请拖入图片文件。"));
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take(20 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(AppError::io)?;
        if bytes.len() > 20 * 1024 * 1024 {
            return Err(AppError::new("image-invalid", "文件超过 20 MiB 上限。"));
        }
        core.import_image(
            &mutation,
            &bytes,
            &path.file_name().unwrap_or_default().to_string_lossy(),
        )
    })
    .await?;
    let core = state.lock().map_err(AppError::io)?;
    renderer.document_changed(core.canvas_stamp(), core.canvas_layers());
    Ok(result)
}

#[tauri::command]
async fn import_image(
    state: tauri::State<'_, Service>,
    window: tauri::WebviewWindow,
    mutation: MutationRequest,
) -> AppResult<Option<itasha_contracts::ImageAsset>> {
    let service = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        {
            service
                .lock()
                .map_err(AppError::io)?
                .image_import_check(&mutation)?;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_parent(&window)
            .set_title("导入图片素材")
            .add_filter("图片", &["png", "jpg", "jpeg", "webp"])
            .pick_file()
        else {
            return Ok(None);
        };
        if path.metadata().map_err(AppError::io)?.len() > 20 * 1024 * 1024 {
            return Err(AppError::new("image-invalid", "文件超过 20 MiB 上限。"));
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let bytes = std::fs::read(path).map_err(AppError::io)?;
        let mut core = service.lock().map_err(AppError::io)?;
        let asset = core.import_image(&mutation, &bytes, &name)?;
        window
            .app_handle()
            .state::<viewport::NativeViewport>()
            .document_changed(core.canvas_stamp(), core.canvas_layers());
        Ok(Some(asset))
    })
    .await
    .map_err(AppError::io)?
}
#[tauri::command]
async fn asset_bytes(
    state: tauri::State<'_, Service>,
    session: SessionRef,
    id: String,
) -> AppResult<tauri::ipc::Response> {
    blocking(state.inner().clone(), move |app| {
        app.image_bytes(&session, &id)
            .map(tauri::ipc::Response::new)
    })
    .await
}
#[tauri::command]
async fn viewport_surface(
    state: tauri::State<'_, Service>,
    renderer: tauri::State<'_, viewport::NativeViewport>,
    session: SessionRef,
    id: String,
) -> AppResult<tauri::ipc::Response> {
    let epoch = session.session_epoch.to_string();
    blocking(state.inner().clone(), move |app| {
        app.session_check(&session, None)
    })
    .await?;
    renderer.mask(&epoch, &id).map(tauri::ipc::Response::new)
}
#[tauri::command]
async fn viewport_coverage(
    state: tauri::State<'_, Service>,
    renderer: tauri::State<'_, viewport::NativeViewport>,
    session: SessionRef,
    mut layer: itasha_contracts::Layer,
) -> AppResult<Vec<itasha_surface_map::CoverageFragment>> {
    let epoch = session.session_epoch.to_string();
    blocking(state.inner().clone(), move |app| {
        app.session_check(&session, None)
    })
    .await?;
    renderer.prepare_layer(&epoch, &mut layer)?;
    renderer.coverage(&epoch, &layer)
}
#[tauri::command]
async fn viewport_transport(
    state: tauri::State<'_, Service>,
    renderer: tauri::State<'_, viewport::NativeViewport>,
    session: SessionRef,
    mut layer: itasha_contracts::Layer,
    surface_id: String,
    point: [f32; 2],
    offset: [f32; 2],
) -> AppResult<itasha_contracts::Layer> {
    let epoch = session.session_epoch.to_string();
    blocking(state.inner().clone(), move |app| {
        app.session_check(&session, None)
    })
    .await?;
    renderer.prepare_layer(&epoch, &mut layer)?;
    let mut result = renderer.transport_layer(&epoch, &layer, surface_id, point, offset)?;
    renderer.prepare_layer(&epoch, &mut result)?;
    Ok(result)
}
#[tauri::command]
async fn viewport_decal(
    request: tauri::ipc::Request<'_>,
    state: tauri::State<'_, Service>,
    renderer: tauri::State<'_, viewport::NativeViewport>,
) -> AppResult<()> {
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| AppError::new("invalid-input", "画布预览缺少版本信息。"))
    };
    let session = SessionRef {
        project_id: header("x-project")?.parse().map_err(AppError::io)?,
        session_epoch: header("x-session")?.parse().map_err(AppError::io)?,
    };
    let epoch = session.session_epoch.to_string();
    let revision = header("x-revision")?;
    let id = header("x-surface").unwrap_or_default();
    let width: u32 = header("x-width")?.parse().map_err(AppError::io)?;
    let height: u32 = header("x-height")?.parse().map_err(AppError::io)?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(AppError::new("invalid-input", "需要二进制 RGBA 数据。"));
    };
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(AppError::new("invalid-input", "画布预览超出预算。"));
    }
    let pixels = bytes.clone();
    let number = revision.parse::<u64>().map_err(AppError::io)?;
    blocking(state.inner().clone(), move |app| {
        app.session_check(&session, Some(&revision))
    })
    .await?;
    let reply = if header("x-layer").is_ok() {
        renderer.artwork(epoch, number, header("x-layer")?, width, height, pixels)?
    } else {
        renderer.decal(epoch, id, number, width, height, pixels)?
    };
    let applied = tauri::async_runtime::spawn_blocking(move || {
        reply.recv_timeout(std::time::Duration::from_secs(5))
    })
    .await
    .map_err(AppError::io)?
    .map_err(AppError::io)?;
    if !applied {
        return Err(AppError::new(
            "preview-superseded",
            "预览已被更新的工程状态替代。",
        ));
    }
    Ok(())
}

#[tauri::command]
async fn viewport_load(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Service>,
    renderer: tauri::State<'_, viewport::NativeViewport>,
    session: SessionRef,
    force: Option<bool>,
) -> AppResult<bool> {
    let epoch = session.session_epoch.to_string();
    let (plan, settings, upgraded) = blocking(state.inner().clone(), move |app| {
        let view = app.view()?;
        let plan = model_package::plan(&view, &session)?;
        // Each reviewed predecessor upgrades independently; every earlier
        // triangle is preserved verbatim in the current map.
        let mut upgraded = false;
        if let Some(fingerprint) = view
            .project
            .as_ref()
            .and_then(|p| p.editor.model_fingerprint.clone())
            && model_package::verified_seam_upgrade(&plan, Some(&fingerprint))?
        {
            upgraded = app.rebind_surface_connections(
                &session,
                &fingerprint,
                model_package::CURRENT_PAINT,
            )?;
        }
        Ok((plan, view.settings, upgraded))
    })
    .await?;
    if upgraded {
        let app = state.lock().map_err(AppError::io)?;
        renderer.document_changed(app.canvas_stamp(), app.canvas_layers());
        window
            .emit("paint-mapping-updated", &epoch)
            .map_err(AppError::io)?;
    }
    renderer.configure(settings.performance, settings.scene)?;
    renderer.load(epoch, plan, force.unwrap_or(false))?;
    Ok(upgraded)
}
#[tauri::command]
fn viewport_bounds(
    renderer: tauri::State<'_, viewport::NativeViewport>,
    bounds: viewport::Bounds,
) -> AppResult<()> {
    renderer.bounds(bounds)
}
#[tauri::command]
fn viewport_status(
    renderer: tauri::State<'_, viewport::NativeViewport>,
) -> AppResult<viewport::Status> {
    renderer.status()
}
#[tauri::command]
async fn viewport_select(
    state: tauri::State<'_, Service>,
    renderer: tauri::State<'_, viewport::NativeViewport>,
    session: SessionRef,
    id: Option<String>,
) -> AppResult<()> {
    let epoch = session.session_epoch.to_string();
    blocking(state.inner().clone(), move |app| {
        app.session_check(&session, None)
    })
    .await?;
    renderer.select(epoch, id)
}
#[tauri::command]
async fn viewport_probe(
    renderer: tauri::State<'_, viewport::NativeViewport>,
    x: f32,
    y: f32,
) -> AppResult<serde_json::Value> {
    let receiver = renderer.probe(x, y)?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver.recv_timeout(std::time::Duration::from_secs(2))
    })
    .await
    .map_err(AppError::io)?
    .map_err(AppError::io)
}
#[tauri::command]
async fn viewport_gesture(
    renderer: tauri::State<'_, viewport::NativeViewport>,
    input: viewport::GestureInput,
) -> AppResult<()> {
    let receiver = renderer.gesture(input)?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver.recv_timeout(std::time::Duration::from_secs(3))
    })
    .await
    .map_err(AppError::io)?
    .map_err(AppError::io)?
}
#[tauri::command]
fn viewport_pointer(
    renderer: tauri::State<'_, viewport::NativeViewport>,
    input: viewport::PointerInput,
) -> AppResult<()> {
    renderer.pointer(input)
}
#[tauri::command]
fn viewport_focus(renderer: tauri::State<'_, viewport::NativeViewport>) -> AppResult<()> {
    renderer.focus()
}
#[tauri::command]
fn viewport_reset(renderer: tauri::State<'_, viewport::NativeViewport>) -> AppResult<()> {
    renderer.reset()
}

type Service = Arc<Mutex<Application>>;
fn native_theme(theme: Theme) -> Option<tauri::Theme> {
    match theme {
        Theme::Dark => Some(tauri::Theme::Dark),
        Theme::Light => Some(tauri::Theme::Light),
        Theme::System => None,
    }
}
#[tauri::command]
async fn choose_directory(window: tauri::WebviewWindow) -> AppResult<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        rfd::FileDialog::new()
            .set_parent(&window)
            .set_title("选择目录")
            .pick_folder()
            .map(|path| path.to_string_lossy().into_owned())
    })
    .await
    .map_err(AppError::io)
}
async fn blocking<T: Send + 'static>(
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
#[tauri::command]
async fn read_view(state: tauri::State<'_, Service>) -> AppResult<View> {
    blocking(state.inner().clone(), |app| app.view()).await
}
#[tauri::command]
async fn execute(
    state: tauri::State<'_, Service>,
    window: tauri::WebviewWindow,
    renderer: tauri::State<'_, viewport::NativeViewport>,
    request_id: Uuid,
    mut action: Action,
) -> AppResult<()> {
    if let Action::Edit {
        mutation,
        operation:
            itasha_contracts::EditOperation::Insert { layer, .. }
            | itasha_contracts::EditOperation::Update { layer },
    } = &mut action
    {
        renderer.prepare_layer(&mutation.session.session_epoch.to_string(), layer)?;
    }
    if let Action::Edit {
        mutation,
        operation: itasha_contracts::EditOperation::Insert { fingerprint, .. },
    } = &action
    {
        let status = renderer.status()?;
        if status.session_epoch != mutation.session.session_epoch.to_string()
            || status.state != "ready"
            || status
                .stats
                .as_ref()
                .is_none_or(|s| s.surface_fingerprint != *fingerprint)
        {
            return Err(AppError::new(
                "model-mismatch",
                "请等待对应车型外板映射就绪，或检查模型版本。",
            ));
        }
    }
    let render_settings = match &action {
        Action::Settings { value } => Some((value.performance.clone(), value.scene.clone())),
        _ => None,
    };
    let theme = match &action {
        Action::Settings { value } => Some(value.theme),
        _ => None,
    };
    let app_handle = window.app_handle().clone();
    blocking(state.inner().clone(), move |app| {
        app.execute(request_id, action)?;
        app_handle
            .state::<viewport::NativeViewport>()
            .document_changed(app.canvas_stamp(), app.canvas_layers());
        Ok(())
    })
    .await?;
    if let Some((performance, scene)) = render_settings {
        renderer.configure(performance, scene)?;
    }
    if let Some(theme) = theme {
        window
            .set_theme(native_theme(theme))
            .map_err(AppError::io)?;
    }
    Ok(())
}
#[tauri::command]
async fn close_clean(
    state: tauri::State<'_, Service>,
    window: tauri::WebviewWindow,
) -> AppResult<()> {
    blocking(state.inner().clone(), move |app| {
        if app.dirty() {
            return Err(AppError::new("unsaved-changes", "请先保存或放弃修改。"));
        }
        window.destroy().map_err(AppError::io)
    })
    .await
}
fn main() {
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
        .invoke_handler(tauri::generate_handler![
            read_view,
            execute,
            close_clean,
            choose_directory,
            viewport_load,
            viewport_bounds,
            viewport_status,
            viewport_reset,
            viewport_focus,
            viewport_select,
            viewport_pointer,
            viewport_gesture,
            viewport_probe,
            import_image,
            import_image_path,
            asset_bytes,
            viewport_surface,
            viewport_coverage,
            viewport_transport,
            viewport_decal
        ])
        .run(tauri::generate_context!())
        .expect("Unable to start desktop shell");
}
