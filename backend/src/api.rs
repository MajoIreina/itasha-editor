//! Version-one editor API. Keep wire compatibility while changing implementations.
use crate::{AppState, export, images, storage};
use anyhow::{Context, Result, ensure};
use serde_json::Value;

pub fn dispatch(state: &AppState, route: &str, args: &Value) -> Result<Value> {
    match route {
        "vehicle-state" => Ok(state.vehicles.list()),
        "performance" => Ok(state.settings.state()),
        "performance-update" => state.settings.update(args),
        "projects" => storage::list_projects(&state.data),
        "open" => storage::open_project(
            &state.data,
            args["file"].as_str().context("缺少工程文件名")?,
        ),
        "save" => {
            let _guard = state.writes.lock().unwrap();
            storage::save_project(&state.data, &args["project"])
        }
        "export" => {
            let _guard = state.writes.lock().unwrap();
            export::export(&state.data, args)
        }
        "decode-image" => {
            let name = args["name"].as_str().context("缺少素材名称")?;
            let payload = images::decode_data(args["data"].as_str().context("缺少素材内容")?)?;
            state.images.decode(&state.settings, name, &payload)
        }
        "import" => {
            let input = args["path"]
                .as_str()
                .context("缺少素材路径")?
                .trim()
                .trim_matches(['\"', '\'']);
            let path = std::path::Path::new(input);
            ensure!(path.is_file(), "找不到该文件，请输入完整的图片文件路径");
            ensure!(
                path.metadata()?.len() <= images::MAX_BYTES as u64,
                "单张素材请控制在 48 MB 以内"
            );
            let name = path
                .file_name()
                .and_then(|v| v.to_str())
                .context("素材文件名无效")?;
            state
                .images
                .decode(&state.settings, name, &std::fs::read(path)?)
        }
        _ => anyhow::bail!("未知操作"),
    }
}
