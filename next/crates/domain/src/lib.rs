use itasha_contracts::{
    AppError, AppResult, ProjectSnapshot, SessionRef, VehicleConfiguration, VehicleDefinition,
};
use itasha_contracts::{EditOperation, EditorDocument};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn vehicle_catalog() -> &'static Vec<VehicleDefinition> {
    static CATALOG: std::sync::OnceLock<Vec<VehicleDefinition>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../../catalog/vehicles.json"))
            .expect("Bundled vehicle catalog is invalid")
    })
}
pub fn validate_configuration(vehicle_id: &str, config: &VehicleConfiguration) -> AppResult<()> {
    let vehicle = vehicle_catalog()
        .iter()
        .find(|v| v.id == vehicle_id)
        .ok_or_else(|| AppError::new("vehicle-unsupported", "此车型尚未注册。"))?;
    if config.catalog_version != 1 {
        return Err(AppError::new(
            "schema-unsupported",
            "选配目录版本不受支持。",
        ));
    }
    let groups = [
        (&vehicle.paint, &config.paint),
        (&vehicle.wheels, &config.wheels),
        (&vehicle.interior, &config.interior),
        (&vehicle.steering, &config.steering),
    ];
    // Captured exclusions describe website purchase rules. Local design options
    // are independent axes; only catalog membership and category are enforced.
    for (options, id) in groups {
        if !options.iter().any(|option| option.id == *id) {
            return Err(AppError::new(
                "invalid-configuration",
                "选配项不属于此车型或类别。",
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectDocument {
    pub format: String,
    pub document_version: u32,
    pub project_id: Uuid,
    pub name: String,
    pub revision: String,
    pub content_id: Uuid,
    pub vehicle_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<VehicleConfiguration>,
    #[serde(default, skip_serializing_if = "EditorDocument::is_empty")]
    pub editor: EditorDocument,
}

impl ProjectDocument {
    pub fn new(name: &str) -> AppResult<Self> {
        Ok(Self {
            format: "itasha-project".into(),
            document_version: 3,
            project_id: Uuid::new_v4(),
            name: validate_name(name)?,
            revision: "0".into(),
            content_id: Uuid::new_v4(),
            vehicle_id: None,
            configuration: None,
            editor: EditorDocument::default(),
        })
    }
    pub fn configured(
        name: &str,
        vehicle_id: String,
        configuration: VehicleConfiguration,
    ) -> AppResult<Self> {
        validate_configuration(&vehicle_id, &configuration)?;
        let mut document = Self::new(name)?;
        document.vehicle_id = Some(vehicle_id);
        document.configuration = Some(configuration);
        Ok(document)
    }
    pub fn validate(&self) -> AppResult<()> {
        if self.format != "itasha-project" || self.document_version != 3 {
            return Err(AppError::new("schema-unsupported", "不支持此工程版本。"));
        }
        if validate_name(&self.name)? != self.name {
            return Err(AppError::new("project-corrupt", "工程名称无效。"));
        }
        match (&self.vehicle_id, &self.configuration) {
            (Some(id), Some(config)) => validate_configuration(id, config)?,
            (None, None) => {}
            _ => {
                return Err(AppError::new(
                    "project-corrupt",
                    "工程车型与选配信息不完整。",
                ));
            }
        }
        self.revision
            .parse::<u64>()
            .map_err(|_| AppError::new("project-corrupt", "工程版本序号无效。"))?;
        self.editor.validate(self.vehicle_id.as_deref())?;
        Ok(())
    }
}

pub fn validate_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(AppError::new(
            "invalid-input",
            "工程名称应为 1–100 个字符，且不能包含控制字符。",
        ));
    }
    Ok(name.to_string())
}

pub struct Session {
    pub document: ProjectDocument,
    pub epoch: Uuid,
    pub saved_content_id: Option<Uuid>,
    history: Vec<(String, EditorDocument, Uuid)>,
    future: Vec<(String, EditorDocument, Uuid)>,
}

impl Session {
    pub fn new(document: ProjectDocument, saved: bool) -> Self {
        let saved_content_id = saved.then_some(document.content_id);
        Self {
            document,
            epoch: Uuid::new_v4(),
            saved_content_id,
            history: vec![],
            future: vec![],
        }
    }
    pub fn snapshot(&self) -> ProjectSnapshot {
        ProjectSnapshot {
            session: SessionRef {
                project_id: self.document.project_id,
                session_epoch: self.epoch,
            },
            name: self.document.name.clone(),
            revision: self.document.revision.clone(),
            content_id: self.document.content_id,
            dirty: self.saved_content_id != Some(self.document.content_id),
            stage: "draft".into(),
            vehicle_id: self.document.vehicle_id.clone(),
            configuration: self.document.configuration.clone(),
            editor: self.document.editor.clone(),
        }
    }
    pub fn check(&self, session: &SessionRef, revision: &str) -> AppResult<()> {
        if session.project_id != self.document.project_id || session.session_epoch != self.epoch {
            return Err(AppError::new(
                "session-expired",
                "工程会话已变化，请重新读取当前工程。",
            ));
        }
        if revision != self.document.revision {
            return Err(AppError::new(
                "revision-conflict",
                "工程已更新，请刷新后再操作。",
            ));
        }
        Ok(())
    }
    fn bump(&mut self) -> AppResult<()> {
        let revision = self
            .document
            .revision
            .parse::<u64>()
            .map_err(|_| AppError::new("project-corrupt", "工程序号无效。"))?;
        self.document.revision = revision
            .checked_add(1)
            .ok_or_else(|| AppError::new("revision-conflict", "工程序号已达上限。"))?
            .to_string();
        Ok(())
    }
    pub fn rename(&mut self, name: &str) -> AppResult<()> {
        let name = validate_name(name)?;
        self.commit(name, self.document.editor.clone())
    }
    /// Host-only upgrade after proving that the authored surface geometry is unchanged.
    /// Rebind history too: undo must never restore an obsolete mapping fingerprint.
    pub fn rebind_surface_connections(&mut self, from: &str, to: &str) -> AppResult<bool> {
        self.remap_surface_mapping(from, to, |_| Ok(()))
    }

    /// Apply a host-verified geometric migration atomically to the document and
    /// every undo/redo state. A failed anchor must leave the entire session intact.
    pub fn remap_surface_mapping(
        &mut self,
        from: &str,
        to: &str,
        mut remap: impl FnMut(&mut itasha_contracts::Layer) -> AppResult<()>,
    ) -> AppResult<bool> {
        if self.document.editor.model_fingerprint.as_deref() != Some(from) || from == to {
            return Ok(false);
        }
        if !itasha_contracts::valid_hash(from) || !itasha_contracts::valid_hash(to) {
            return Err(AppError::new("model-mismatch", "漆面接缝升级指纹无效。"));
        }
        let mut rebind = |editor: &mut EditorDocument| -> AppResult<()> {
            if editor.model_fingerprint.as_deref() == Some(from) {
                editor.model_fingerprint = Some(to.into());
                for layer in &mut editor.layers {
                    remap(layer)?;
                    if let Some(anchor) = &mut layer.placement.anchor {
                        anchor.paint_surface_fingerprint = to.into();
                    }
                }
            }
            editor.validate(self.document.vehicle_id.as_deref())
        };
        let mut editor = self.document.editor.clone();
        let mut history = self.history.clone();
        let mut future = self.future.clone();
        rebind(&mut editor)?;
        for (_, editor, content_id) in history.iter_mut().chain(future.iter_mut()) {
            rebind(editor)?;
            *content_id = Uuid::new_v4();
        }
        self.bump()?;
        self.document.editor = editor;
        self.document.content_id = Uuid::new_v4();
        self.history = history;
        self.future = future;
        Ok(true)
    }
    fn commit(&mut self, name: String, editor: EditorDocument) -> AppResult<()> {
        if name == self.document.name && editor == self.document.editor {
            return Ok(());
        }
        self.bump()?;
        self.history.push((
            self.document.name.clone(),
            self.document.editor.clone(),
            self.document.content_id,
        ));
        // Bound both entry count and retained serialized metadata; image bytes live in blobs.
        while self.history.len() > 100
            || (self.history.len() > 1
                && self
                    .history
                    .iter()
                    .map(|(_, e, _)| serde_json::to_vec(e).map(|b| b.len()).unwrap_or(0))
                    .sum::<usize>()
                    > 16 * 1024 * 1024)
        {
            self.history.remove(0);
        }
        self.future.clear();
        self.document.name = name;
        self.document.editor = editor;
        self.document.content_id = Uuid::new_v4();
        Ok(())
    }
    pub fn edit(&mut self, operation: EditOperation) -> AppResult<()> {
        let mut editor = self.document.editor.clone();
        let missing = || AppError::new("layer-missing", "目标图层不存在。");
        let locked = || AppError::new("layer-locked", "请先解锁图层。");
        match operation {
            EditOperation::Insert { layer, fingerprint } => {
                if editor
                    .model_fingerprint
                    .as_ref()
                    .is_some_and(|v| *v != fingerprint)
                {
                    return Err(AppError::new(
                        "model-mismatch",
                        "模型几何已变化，禁止将旧图层映射到新拓扑。",
                    ));
                }
                editor.model_fingerprint = Some(fingerprint);
                editor.layers.push(layer);
            }
            EditOperation::Update { layer } => {
                let old = editor
                    .layers
                    .iter_mut()
                    .find(|v| v.id == layer.id)
                    .ok_or_else(missing)?;
                if old.locked {
                    let mut unlocked = old.clone();
                    unlocked.locked = false;
                    if layer != unlocked && &layer != old {
                        return Err(locked());
                    }
                }
                *old = layer;
            }
            EditOperation::Delete { id } => {
                let index = editor
                    .layers
                    .iter()
                    .position(|v| v.id == id)
                    .ok_or_else(missing)?;
                if editor.layers[index].locked {
                    return Err(locked());
                }
                editor.layers.remove(index);
            }
            EditOperation::Reorder { id, offset } => {
                if ![-1, 1].contains(&offset) {
                    return Err(AppError::new("invalid-input", "排序步长无效。"));
                }
                let index = editor
                    .layers
                    .iter()
                    .position(|v| v.id == id)
                    .ok_or_else(missing)?;
                if editor.layers[index].locked {
                    return Err(locked());
                }
                let target =
                    (index as i32 + offset).clamp(0, editor.layers.len() as i32 - 1) as usize;
                let item = editor.layers.remove(index);
                editor.layers.insert(target, item);
            }
        }
        editor.validate(self.document.vehicle_id.as_deref())?;
        self.commit(self.document.name.clone(), editor)
    }
    pub fn add_asset(&mut self, asset: itasha_contracts::ImageAsset) -> AppResult<()> {
        let mut editor = self.document.editor.clone();
        if !editor.assets.iter().any(|v| v.id == asset.id) {
            editor.assets.push(asset)
        }
        editor.validate(self.document.vehicle_id.as_deref())?;
        self.commit(self.document.name.clone(), editor)
    }
    pub fn undo(&mut self, redo: bool) -> AppResult<()> {
        let item = if redo {
            self.future.last()
        } else {
            self.history.last()
        }
        .cloned();
        let Some((name, editor, content_id)) = item else {
            return Ok(());
        };
        self.bump()?;
        let current = (
            self.document.name.clone(),
            self.document.editor.clone(),
            self.document.content_id,
        );
        if redo {
            self.future.pop();
            self.history.push(current);
        } else {
            self.history.pop();
            self.future.push(current);
        }
        self.document.name = name;
        self.document.editor = editor;
        self.document.content_id = content_id;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn layer() -> itasha_contracts::Layer {
        use itasha_contracts::*;
        Layer {
            id: Uuid::new_v4(),
            name: "矩形".into(),
            visible: true,
            locked: false,
            opacity: 1.,
            placement: Placement {
                anchor: Some(itasha_contracts::SurfaceAnchor {
                    part_id: "su7.front-left-door".into(),
                    region_id: "outer".into(),
                    source_triangle_id: "test:0:0:0".into(),
                    barycentric: [1., 0., 0.],
                    paint_surface_fingerprint: "a".repeat(64),
                }),
                cross_surface: false,
                flip_x: false,
                aspect_ratio_locked: false,
                surface_id: "su7.front-left-door".into(),
                profile_version: 2,
                x_mm: 500.,
                y_mm: 300.,
                width_mm: 200.,
                height_mm: 120.,
                rotation_deg: 0.,
            },
            source: LayerSource::Shape {
                shape: Shape::Rectangle,
                fill: "#0078d4".into(),
            },
        }
    }
    #[test]
    fn connection_upgrade_preserves_edits_and_rebinds_undo_redo() {
        let vehicle = vehicle_catalog().iter().find(|v| v.id == "su7").unwrap();
        let mut session = Session::new(
            ProjectDocument::configured("接缝", vehicle.id.clone(), vehicle.defaults.clone())
                .unwrap(),
            true,
        );
        let original = layer();
        session
            .edit(EditOperation::Insert {
                layer: original.clone(),
                fingerprint: "a".repeat(64),
            })
            .unwrap();
        let mut moved = original.clone();
        moved.placement.x_mm += 42.;
        moved.placement.aspect_ratio_locked = true;
        session
            .edit(EditOperation::Update {
                layer: moved.clone(),
            })
            .unwrap();
        session.undo(false).unwrap();
        let revision = session.document.revision.clone();
        assert!(
            session
                .rebind_surface_connections(&"a".repeat(64), "invalid")
                .is_err()
        );
        assert_eq!(session.document.revision, revision);
        assert!(
            session
                .rebind_surface_connections(&"a".repeat(64), &"b".repeat(64))
                .unwrap()
        );
        assert!(
            !session
                .rebind_surface_connections(&"a".repeat(64), &"b".repeat(64))
                .unwrap()
        );
        assert!(session.snapshot().dirty);
        session.undo(true).unwrap();
        moved
            .placement
            .anchor
            .as_mut()
            .unwrap()
            .paint_surface_fingerprint = "b".repeat(64);
        assert_eq!(session.document.editor.layers, vec![moved]);
        session.undo(false).unwrap();
        assert_eq!(
            session.document.editor.layers[0].placement.x_mm,
            original.placement.x_mm
        );
        assert_eq!(
            session.document.editor.model_fingerprint.as_deref(),
            Some("b".repeat(64).as_str())
        );
        session.document.validate().unwrap();
    }
    #[test]
    fn geometric_upgrade_is_atomic_and_reprojects_undo_redo() {
        let vehicle = vehicle_catalog().iter().find(|v| v.id == "su7").unwrap();
        let mut session = Session::new(
            ProjectDocument::configured("迁移", vehicle.id.clone(), vehicle.defaults.clone())
                .unwrap(),
            true,
        );
        let original = layer();
        session
            .edit(EditOperation::Insert {
                layer: original.clone(),
                fingerprint: "a".repeat(64),
            })
            .unwrap();
        let mut moved = original.clone();
        moved.placement.x_mm += 42.;
        session
            .edit(EditOperation::Update {
                layer: moved.clone(),
            })
            .unwrap();
        session.undo(false).unwrap();
        let before = serde_json::to_value(&session.document).unwrap();
        let history = session.history.clone();
        let future = session.future.clone();
        let result = session.remap_surface_mapping(&"a".repeat(64), &"b".repeat(64), |layer| {
            if layer.placement.x_mm == moved.placement.x_mm {
                return Err(AppError::new("model-mismatch", "unmapped history anchor"));
            }
            layer.placement.x_mm += 100.;
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(serde_json::to_value(&session.document).unwrap(), before);
        assert_eq!(session.history, history);
        assert_eq!(session.future, future);
        session
            .remap_surface_mapping(&"a".repeat(64), &"b".repeat(64), |layer| {
                layer.placement.x_mm += 100.;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            session.document.editor.layers[0].placement.x_mm,
            original.placement.x_mm + 100.
        );
        session.undo(true).unwrap();
        assert_eq!(
            session.document.editor.layers[0].placement.x_mm,
            moved.placement.x_mm + 100.
        );
        session.undo(false).unwrap();
        assert_eq!(
            session.document.editor.layers[0].placement.x_mm,
            original.placement.x_mm + 100.
        );
        assert_eq!(
            session.document.editor.layers[0].placement.width_mm,
            original.placement.width_mm
        );
        assert_eq!(
            session.document.editor.layers[0]
                .placement
                .anchor
                .as_ref()
                .unwrap()
                .paint_surface_fingerprint,
            "b".repeat(64)
        );
    }

    #[test]
    fn canvas_changes_are_atomic_and_restore_saved_content() {
        let vehicle = vehicle_catalog().iter().find(|v| v.id == "su7").unwrap();
        let mut session = Session::new(
            ProjectDocument::configured("图层", vehicle.id.clone(), vehicle.defaults.clone())
                .unwrap(),
            true,
        );
        let before = session.snapshot();
        let a = layer();
        let hash = "a".repeat(64);
        session
            .edit(EditOperation::Insert {
                layer: a.clone(),
                fingerprint: hash.clone(),
            })
            .unwrap();
        let mut moved = a.clone();
        moved.placement.x_mm += 30.;
        moved.placement.rotation_deg = 45.;
        session
            .edit(EditOperation::Update {
                layer: moved.clone(),
            })
            .unwrap();
        session.undo(false).unwrap();
        assert_eq!(session.document.editor.layers[0], a);
        session.undo(false).unwrap();
        assert!(!session.snapshot().dirty);
        assert_eq!(session.snapshot().revision, "4");
        assert_eq!(session.document.editor, before.editor);
        session.undo(true).unwrap();
        assert_eq!(session.document.editor.layers[0], a);
        session.undo(true).unwrap();
        assert_eq!(session.document.editor.layers[0], moved);
        let snapshot = session.snapshot();
        moved.placement.x_mm = f64::NAN;
        assert!(
            session
                .edit(EditOperation::Update { layer: moved })
                .is_err()
        );
        assert_eq!(session.snapshot(), snapshot);
        assert!(
            session
                .edit(EditOperation::Insert {
                    layer: layer(),
                    fingerprint: "b".repeat(64)
                })
                .is_err()
        );
        assert_eq!(session.snapshot(), snapshot);
    }
    #[test]
    fn only_registered_outer_parts_and_unlocked_layers_can_change() {
        let vehicle = vehicle_catalog().iter().find(|v| v.id == "su7").unwrap();
        let mut session = Session::new(
            ProjectDocument::configured("图层", vehicle.id.clone(), vehicle.defaults.clone())
                .unwrap(),
            false,
        );
        let mut a = layer();
        a.placement.surface_id = "glass".into();
        assert!(
            session
                .edit(EditOperation::Insert {
                    layer: a.clone(),
                    fingerprint: "a".repeat(64)
                })
                .is_err()
        );
        a.placement.surface_id = "su7.front-left-door".into();
        session
            .edit(EditOperation::Insert {
                layer: a.clone(),
                fingerprint: "a".repeat(64),
            })
            .unwrap();
        a.locked = true;
        session
            .edit(EditOperation::Update { layer: a.clone() })
            .unwrap();
        let snapshot = session.snapshot();
        assert!(session.edit(EditOperation::Delete { id: a.id }).is_err());
        a.placement.x_mm += 10.;
        assert!(
            session
                .edit(EditOperation::Update { layer: a.clone() })
                .is_err()
        );
        assert_eq!(session.snapshot(), snapshot);
        a.placement.x_mm -= 10.;
        a.locked = false;
        session
            .edit(EditOperation::Update { layer: a.clone() })
            .unwrap();
        session.edit(EditOperation::Delete { id: a.id }).unwrap();
        assert!(session.document.editor.layers.is_empty());
    }
    #[test]
    fn front_and_rear_c_rings_are_independent_paint_surfaces() {
        let vehicle = vehicle_catalog().iter().find(|v| v.id == "su7").unwrap();
        let mut session = Session::new(
            ProjectDocument::configured("C 环", vehicle.id.clone(), vehicle.defaults.clone())
                .unwrap(),
            false,
        );
        for id in ["su7.front-c-ring", "su7.rear-c-ring"] {
            let mut part = layer();
            part.placement.surface_id = id.into();
            part.placement.anchor.as_mut().unwrap().part_id = id.into();
            session
                .edit(EditOperation::Insert {
                    layer: part,
                    fingerprint: "a".repeat(64),
                })
                .unwrap();
        }
        assert_eq!(session.document.editor.layers.len(), 2);
        session.undo(false).unwrap();
        assert_eq!(
            session.document.editor.layers[0].placement.surface_id,
            "su7.front-c-ring"
        );
        session.undo(true).unwrap();
        assert_eq!(
            session.document.editor.layers[1].placement.surface_id,
            "su7.rear-c-ring"
        );
    }
    #[test]
    fn every_registered_vehicle_has_valid_default_and_rejects_foreign_options() {
        assert_eq!(vehicle_catalog().len(), 7);
        for vehicle in vehicle_catalog() {
            validate_configuration(&vehicle.id, &vehicle.defaults).unwrap();
            let mut config = vehicle.defaults.clone();
            config.paint = "foreign-id".into();
            assert_eq!(
                validate_configuration(&vehicle.id, &config)
                    .unwrap_err()
                    .code,
                "invalid-configuration"
            );
        }
    }
    #[test]
    fn every_registered_option_combination_is_independent() {
        for vehicle in vehicle_catalog() {
            for paint in &vehicle.paint {
                for wheels in &vehicle.wheels {
                    for interior in &vehicle.interior {
                        for steering in &vehicle.steering {
                            let config = VehicleConfiguration {
                                catalog_version: 1,
                                paint: paint.id.clone(),
                                wheels: wheels.id.clone(),
                                interior: interior.id.clone(),
                                steering: steering.id.clone(),
                            };
                            validate_configuration(&vehicle.id, &config).unwrap_or_else(|error| {
                                panic!("{} {config:?}: {error:?}", vehicle.id)
                            });
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn previously_restricted_su7_options_survive_document_round_trip() {
        let vehicle = vehicle_catalog().iter().find(|v| v.id == "su7").unwrap();
        // Red and black were incorrectly blocked by the default purple interior;
        // the 20-inch rice-pattern wheel also carried website paint restrictions.
        for paint in ["600061064", "600061063", "600061068"] {
            for wheels in &vehicle.wheels {
                let config = VehicleConfiguration {
                    paint: paint.into(),
                    wheels: wheels.id.clone(),
                    ..vehicle.defaults.clone()
                };
                let document =
                    ProjectDocument::configured("独立选配", vehicle.id.clone(), config.clone())
                        .unwrap();
                let restored: ProjectDocument =
                    serde_json::from_str(&serde_json::to_string(&document).unwrap()).unwrap();
                restored.validate().unwrap();
                assert_eq!(restored.configuration, Some(config));
            }
        }
    }
    #[test]
    fn configuration_identity_category_and_version_are_still_validated() {
        let vehicle = vehicle_catalog().iter().find(|v| v.id == "su7").unwrap();
        assert_eq!(
            validate_configuration("unregistered", &vehicle.defaults)
                .unwrap_err()
                .code,
            "vehicle-unsupported"
        );
        let mut config = vehicle.defaults.clone();
        config.catalog_version = 2;
        assert_eq!(
            validate_configuration("su7", &config).unwrap_err().code,
            "schema-unsupported"
        );
        let mut config = vehicle.defaults.clone();
        config.paint = vehicle.wheels[0].id.clone();
        assert_eq!(
            validate_configuration("su7", &config).unwrap_err().code,
            "invalid-configuration"
        );
    }
    #[test]
    fn incomplete_documents_are_rejected() {
        let mut document = ProjectDocument::new("A").unwrap();
        document.vehicle_id = Some("su7".into());
        assert_eq!(document.validate().unwrap_err().code, "project-corrupt");
    }
    #[test]
    fn invalid_name_does_not_change_document() {
        let mut s = Session::new(ProjectDocument::new("原工程").unwrap(), false);
        let before = s.snapshot();
        assert!(s.rename("\n").is_err());
        assert_eq!(s.snapshot(), before);
    }
    #[test]
    fn undo_restores_saved_content_but_revision_still_increases() {
        let mut s = Session::new(ProjectDocument::new("已保存").unwrap(), true);
        s.rename("修改").unwrap();
        assert!(s.snapshot().dirty);
        s.undo(false).unwrap();
        assert!(!s.snapshot().dirty);
        assert_eq!(s.document.revision, "2");
        s.undo(true).unwrap();
        assert_eq!(s.document.name, "修改");
        assert!(s.snapshot().dirty);
    }
    #[test]
    fn session_epoch_rejects_stale_handles() {
        let a = Session::new(ProjectDocument::new("A").unwrap(), false);
        let b = Session::new(a.document.clone(), true);
        assert_eq!(
            b.check(&a.snapshot().session, "0").unwrap_err().code,
            "session-expired"
        );
    }
}
