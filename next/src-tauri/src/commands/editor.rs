//! Project and settings commands; mutations still pass through the application service.
use crate::host::{Service, blocking, native_theme};
use crate::platform::viewport;
use itasha_application::{Action, View};
use itasha_contracts::{AppError, AppResult};
use tauri::Manager;
use uuid::Uuid;

#[tauri::command]
pub(crate) async fn read_view(state: tauri::State<'_, Service>) -> AppResult<View> {
    blocking(state.inner().clone(), |app| app.view()).await
}

#[tauri::command]
pub(crate) async fn execute(
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
pub(crate) async fn close_clean(
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
