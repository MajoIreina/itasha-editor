//! Image import, current-project asset reads and directory selection.
use crate::host::{Service, blocking};
use crate::platform::viewport;
use itasha_contracts::{AppError, AppResult, MutationRequest, SessionRef};
use tauri::Manager;

#[tauri::command]
pub(crate) async fn import_image_path(
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
pub(crate) async fn import_image(
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
pub(crate) async fn asset_bytes(
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
pub(crate) async fn choose_directory(window: tauri::WebviewWindow) -> AppResult<Option<String>> {
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
