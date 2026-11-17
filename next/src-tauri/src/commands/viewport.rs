//! IPC adapters for the native viewport; rendering remains on its own thread.
use crate::adapters::su7;
use crate::host::{Service, blocking};
use crate::platform::viewport;
use itasha_contracts::{AppError, AppResult, SessionRef};
use tauri::Emitter;

#[tauri::command]
pub(crate) async fn viewport_surface(
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
pub(crate) async fn viewport_coverage(
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
pub(crate) async fn viewport_transport(
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
pub(crate) async fn viewport_decal(
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
pub(crate) async fn viewport_load(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Service>,
    renderer: tauri::State<'_, viewport::NativeViewport>,
    session: SessionRef,
    force: Option<bool>,
) -> AppResult<bool> {
    let epoch = session.session_epoch.to_string();
    let (plan, settings, upgraded) = blocking(state.inner().clone(), move |app| {
        let view = app.view()?;
        let plan = su7::plan(&view, &session)?;
        let mut upgraded = false;
        if let Some(fingerprint) = view
            .project
            .as_ref()
            .and_then(|p| p.editor.model_fingerprint.clone())
        {
            upgraded = su7::upgrade_mapping(app, &plan, &session, &fingerprint)?;
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
pub(crate) fn viewport_bounds(
    renderer: tauri::State<'_, viewport::NativeViewport>,
    bounds: viewport::Bounds,
) -> AppResult<()> {
    renderer.bounds(bounds)
}

#[tauri::command]
pub(crate) fn viewport_status(
    renderer: tauri::State<'_, viewport::NativeViewport>,
) -> AppResult<viewport::Status> {
    renderer.status()
}

#[tauri::command]
pub(crate) async fn viewport_select(
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
pub(crate) async fn viewport_probe(
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
pub(crate) async fn viewport_gesture(
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
pub(crate) fn viewport_pointer(
    renderer: tauri::State<'_, viewport::NativeViewport>,
    input: viewport::PointerInput,
) -> AppResult<()> {
    renderer.pointer(input)
}

#[tauri::command]
pub(crate) fn viewport_focus(
    renderer: tauri::State<'_, viewport::NativeViewport>,
) -> AppResult<()> {
    renderer.focus()
}

#[tauri::command]
pub(crate) fn viewport_reset(
    renderer: tauri::State<'_, viewport::NativeViewport>,
) -> AppResult<()> {
    renderer.reset()
}
