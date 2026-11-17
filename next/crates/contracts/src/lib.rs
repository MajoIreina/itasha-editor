use serde::{Deserialize, Serialize};
use uuid::Uuid;
mod editor;
pub use editor::*;

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
        }
    }
    pub fn io(error: impl std::fmt::Display) -> Self {
        eprintln!("Repository error: {error}");
        Self {
            code: "io-error".into(),
            message: "无法读写本机工程，请检查磁盘空间和目录权限。".into(),
            retryable: true,
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionRef {
    pub project_id: Uuid,
    pub session_epoch: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MutationRequest {
    pub request_id: Uuid,
    pub session: SessionRef,
    pub expected_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRequest {
    pub request_id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameRequest {
    pub mutation: MutationRequest,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSummary {
    pub project_id: Uuid,
    pub name: String,
    pub revision: String,
    pub content_id: Uuid,
    pub saved_at_unix_ms: String,
    pub vehicle_id: Option<String>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSnapshot {
    pub session: SessionRef,
    pub name: String,
    pub revision: String,
    pub content_id: Uuid,
    pub dirty: bool,
    pub stage: String,
    pub vehicle_id: Option<String>,
    pub configuration: Option<VehicleConfiguration>,
    pub editor: EditorDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VehicleConfiguration {
    pub catalog_version: u32,
    pub paint: String,
    pub wheels: String,
    pub interior: String,
    pub steering: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VehicleOption {
    pub id: String,
    pub name: String,
    pub swatch: Option<String>,
    pub excludes: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VehicleDefinition {
    pub id: String,
    pub name: String,
    pub family: String,
    pub body_style: String,
    pub source_sha256: String,
    pub paint: Vec<VehicleOption>,
    pub wheels: Vec<VehicleOption>,
    pub interior: Vec<VehicleOption>,
    pub steering: Vec<VehicleOption>,
    pub defaults: VehicleConfiguration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveResult {
    pub project_id: Uuid,
    pub session_epoch: Uuid,
    pub saved_revision: String,
    pub saved_content_id: Uuid,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub schema_version: u32,
    #[serde(default)]
    pub theme: Theme,
    pub layout: String,
    pub autosave: bool,
    #[serde(default)]
    pub workspace: WorkspaceSettings,
    #[serde(default)]
    pub performance: PerformanceSettings,
    #[serde(default)]
    pub scene: SceneSettings,
    #[serde(default)]
    pub shortcuts: Shortcuts,
    #[serde(default)]
    pub paths: PathSettings,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceSettings {
    pub direction: String,
    pub split_percent: u8,
    #[serde(default)]
    pub panels: std::collections::BTreeMap<String, PanelLayout>,
    #[serde(default = "default_library_width")]
    pub library_width: u16,
    #[serde(default)]
    pub library_minimized: bool,
}
fn default_library_width() -> u16 {
    214
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PanelLayout {
    pub height: u16,
    pub minimized: bool,
}
impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            direction: "vertical".into(),
            split_percent: 65,
            panels: std::collections::BTreeMap::new(),
            library_width: default_library_width(),
            library_minimized: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PerformanceSettings {
    pub target_fps: u16,
    pub render_scale: u16,
    pub texture_limit: u16,
    pub cache_budget_mb: u32,
    pub reduced_motion: bool,
    #[serde(default = "default_antialiasing")]
    pub antialiasing: String,
}
impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            target_fps: 60,
            render_scale: 100,
            texture_limit: 4096,
            cache_budget_mb: 2048,
            reduced_motion: false,
            antialiasing: default_antialiasing(),
        }
    }
}
fn default_antialiasing() -> String {
    "fxaa".into()
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneSettings {
    pub skybox: bool,
    pub time_minutes: u16,
}
impl Default for SceneSettings {
    fn default() -> Self {
        Self {
            skybox: true,
            time_minutes: 840,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Shortcuts {
    pub save: String,
    pub undo: String,
    pub redo: String,
    pub settings: String,
}
impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            save: "Ctrl+S".into(),
            undo: "Ctrl+Z".into(),
            redo: "Ctrl+Shift+Z".into(),
            settings: "Ctrl+Comma".into(),
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PathSettings {
    pub models: String,
    pub exports: String,
    pub cache: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            theme: Theme::default(),
            layout: "both".into(),
            autosave: false,
            workspace: WorkspaceSettings::default(),
            performance: PerformanceSettings::default(),
            scene: SceneSettings::default(),
            shortcuts: Shortcuts::default(),
            paths: PathSettings::default(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> AppResult<()> {
        if self.schema_version != 1 || !["both", "edit", "3d"].contains(&self.layout.as_str()) {
            return Err(AppError::new("invalid-input", "设置版本或工作区布局无效。"));
        }
        if self.autosave {
            return Err(AppError::new(
                "feature-unavailable",
                "阶段 1 尚未接入自动保存，请使用手动保存。",
            ));
        }
        let w = &self.workspace;
        if !(160..=360).contains(&w.library_width) {
            return Err(AppError::new(
                "invalid-input",
                "工程库宽度应在 160–360 像素之间。",
            ));
        }
        for (key, panel) in &w.panels {
            if !["properties", "assets", "layers", "shapes", "text"].contains(&key.as_str())
                || !(100..=800).contains(&panel.height)
            {
                return Err(AppError::new("invalid-input", "右侧面板布局参数无效。"));
            }
        }
        let p = &self.performance;
        if !["vertical", "horizontal"].contains(&w.direction.as_str())
            || !(25..=75).contains(&w.split_percent)
            || ![30, 60, 120].contains(&p.target_fps)
            || !["off", "fxaa", "taa", "taa2x", "ssaa"].contains(&p.antialiasing.as_str())
            || self.scene.time_minutes > 1439
            || ![50, 75, 100].contains(&p.render_scale)
            || ![1024, 2048, 4096, 8192].contains(&p.texture_limit)
            || !(256..=16384).contains(&p.cache_budget_mb)
        {
            return Err(AppError::new(
                "invalid-input",
                "工作区或性能参数超出支持范围。",
            ));
        }
        let keys = [
            &self.shortcuts.save,
            &self.shortcuts.undo,
            &self.shortcuts.redo,
            &self.shortcuts.settings,
        ];
        let mut seen = std::collections::HashSet::new();
        for key in keys {
            let parts: Vec<_> = key.split('+').collect();
            let valid = (parts.len() == 2 || parts.len() == 3)
                && parts[0] == "Ctrl"
                && (parts.len() == 2 || parts[1] == "Shift")
                && parts.last().is_some_and(|last| {
                    *last == "Comma" || (last.len() == 1 && last.as_bytes()[0].is_ascii_uppercase())
                });
            if !valid || !seen.insert(key) {
                return Err(AppError::new(
                    "invalid-input",
                    "快捷键须为 Ctrl（可加 Shift）与字母或逗号的组合，且不能重复。",
                ));
            }
        }
        for value in [&self.paths.models, &self.paths.exports, &self.paths.cache] {
            if value.len() > 4096 || value.chars().any(char::is_control) {
                return Err(AppError::new("invalid-input", "目录路径长度或字符无效。"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    #[test]
    fn rendering_settings_migrate_and_reject_invalid_values() {
        let mut old = serde_json::to_value(Settings::default()).unwrap();
        old.as_object_mut().unwrap().remove("scene");
        old["performance"]
            .as_object_mut()
            .unwrap()
            .remove("antialiasing");
        let mut loaded: Settings = serde_json::from_value(old).unwrap();
        assert_eq!(loaded.performance.antialiasing, "fxaa");
        assert_eq!(loaded.scene.time_minutes, 840);
        loaded.validate().unwrap();
        loaded.performance.antialiasing = "invalid".into();
        assert!(loaded.validate().is_err());
        loaded.performance.antialiasing = "off".into();
        loaded.scene.time_minutes = 1440;
        assert!(loaded.validate().is_err());
        loaded.scene.time_minutes = 0;
        loaded.validate().unwrap();
    }
    #[test]
    fn theme_migrates_old_settings_and_rejects_unknown_values() {
        let mut old = serde_json::to_value(Settings::default()).unwrap();
        old.as_object_mut().unwrap().remove("theme");
        assert_eq!(
            serde_json::from_value::<Settings>(old.clone())
                .unwrap()
                .theme,
            Theme::Dark
        );
        for theme in ["dark", "light", "system"] {
            old["theme"] = serde_json::json!(theme);
            let settings = serde_json::from_value::<Settings>(old.clone()).unwrap();
            assert!(settings.validate().is_ok());
            assert_eq!(serde_json::to_value(settings).unwrap()["theme"], theme);
        }
        old["theme"] = serde_json::json!("unknown");
        assert!(serde_json::from_value::<Settings>(old).is_err());
    }
    #[test]
    fn antialiasing_modes_round_trip_and_preserve_render_scale() {
        for antialiasing in ["off", "fxaa", "taa", "taa2x", "ssaa"] {
            for render_scale in [50, 75, 100] {
                let mut settings = Settings::default();
                settings.performance.antialiasing = antialiasing.into();
                settings.performance.render_scale = render_scale;
                settings.validate().unwrap();
                let restored: Settings =
                    serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
                restored.validate().unwrap();
                assert_eq!(restored, settings);
            }
        }
        for unsupported in ["", "TAA", "taa4x", "msaa", "ssaa4x"] {
            let mut settings = Settings::default();
            settings.performance.antialiasing = unsupported.into();
            assert_eq!(settings.validate().unwrap_err().code, "invalid-input");
        }
    }
    #[test]
    fn settings_reject_conflicts_and_invalid_budgets() {
        let mut s = Settings::default();
        s.shortcuts.undo = s.shortcuts.save.clone();
        assert!(s.validate().is_err());
        s.shortcuts = Shortcuts::default();
        s.performance.cache_budget_mb = 1;
        assert!(s.validate().is_err());
    }
}
