use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

pub fn safe_name(name: &str, fallback: &str) -> String {
    let name: String = name
        .chars()
        .take(100)
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim_matches([' ', '.']);
    if name.is_empty() {
        return fallback.into();
    }
    // Windows device names are invalid even with a file extension.
    let stem = name.split('.').next().unwrap_or("").to_uppercase();
    if [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .contains(&stem.as_str())
    {
        format!("_{name}")
    } else {
        name.into()
    }
}

pub fn child_file(directory: &Path, relative: &str) -> Result<PathBuf> {
    ensure!(!relative.contains(['\\', ':', '\0']), "文件路径无效");
    let relative = Path::new(relative);
    ensure!(
        !relative.is_absolute()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "文件路径无效"
    );
    let base = directory.canonicalize()?;
    let target = base.join(relative).canonicalize().context("找不到文件")?;
    ensure!(
        target.starts_with(&base) && target.is_file(),
        "文件路径无效"
    );
    Ok(target)
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().context("缺少文件目录")?;
    fs::create_dir_all(parent)?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    pending.write_all(data)?;
    pending.as_file().sync_all()?;
    pending
        .persist(path)
        .map_err(|e| anyhow::anyhow!("无法保存文件：{}", e.error))?;
    Ok(())
}

pub fn list_projects(data: &Path) -> Result<Value> {
    let mut rows = Vec::new();
    for entry in fs::read_dir(data.join("projects"))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("su7design") || !path.is_file() {
            continue;
        }
        let modified: chrono::DateTime<chrono::Local> = entry.metadata()?.modified()?.into();
        rows.push(json!({"name":path.file_stem().and_then(|s| s.to_str()),"file":entry.file_name().to_string_lossy(),"modified":modified.format("%Y-%m-%dT%H:%M:%S").to_string()}));
    }
    rows.sort_by(|a, b| b["modified"].as_str().cmp(&a["modified"].as_str()));
    Ok(json!({"projects":rows}))
}

pub fn save_project(data: &Path, project: &Value) -> Result<Value> {
    ensure!(
        project["format"] == "su7-livery" && project["version"] == 1,
        "工程格式错误"
    );
    let name = format!(
        "{}.su7design",
        safe_name(project["name"].as_str().unwrap_or("未命名"), "未命名")
    );
    let target = data.join("projects").join(&name);
    // Never replace an existing user-controlled junction/symlink.
    if fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("工程文件不能是符号链接");
    }
    atomic_write(&target, &serde_json::to_vec(project)?)?;
    Ok(json!({"file":name,"path":target.to_string_lossy()}))
}

pub fn open_project(data: &Path, name: &str) -> Result<Value> {
    ensure!(
        Path::new(name).extension().and_then(|s| s.to_str()) == Some("su7design"),
        "工程路径无效"
    );
    let target = child_file(&data.join("projects"), name)?;
    ensure!(
        target.parent() == Some(data.join("projects").canonicalize()?.as_path()),
        "工程路径无效"
    );
    Ok(json!({"project":serde_json::from_slice::<Value>(&fs::read(target)?)?}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_roundtrip_and_path_boundary() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("projects")).unwrap();
        let p = json!({"format":"su7-livery","version":1,"name":"中文工程","panels":{},"assets":{"a":{"src":"data:image/png;base64,AA=="}}});
        let result = save_project(dir.path(), &p).unwrap();
        assert_eq!(
            open_project(dir.path(), result["file"].as_str().unwrap()).unwrap()["project"],
            p
        );
        assert!(open_project(dir.path(), "../outside.su7design").is_err());
        assert!(save_project(dir.path(), &json!({"format":"bad","version":1})).is_err());
        assert_eq!(safe_name("CON", "x"), "_CON");
        assert_eq!(safe_name("../x", "x"), "_x");
    }
}
