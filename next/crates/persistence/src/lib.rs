use itasha_contracts::{AppError, AppResult, ProjectSummary, Settings};
use itasha_domain::ProjectDocument;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const MAX_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SavedEnvelope {
    storage_version: u32,
    saved_at_unix_ms: String,
    document_sha256: String,
    document: ProjectDocument,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LibraryPreferences {
    pinned_project_ids: BTreeSet<Uuid>,
}

#[derive(Clone)]
pub struct Repository {
    root: PathBuf,
    _lock: Arc<fs::File>,
}

impl Repository {
    pub fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
    pub fn new(root: PathBuf) -> AppResult<Self> {
        fs::create_dir_all(root.join("projects")).map_err(AppError::io)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".workspace.lock"))
            .map_err(AppError::io)?;
        lock.try_lock().map_err(|_| {
            AppError::new(
                "workspace-in-use",
                "此工作目录已被另一实例占用，或无法取得写入锁。",
            )
        })?;
        Ok(Self {
            root,
            _lock: Arc::new(lock),
        })
    }
    fn project_file(&self, id: Uuid) -> PathBuf {
        self.root.join("projects").join(format!("{id}.json"))
    }
    pub fn save(&self, document: &ProjectDocument) -> AppResult<()> {
        document.validate()?;
        for asset in &document.editor.assets {
            self.image_bytes(&asset.id)?;
        }
        let bytes = serde_json::to_vec(document).map_err(AppError::io)?;
        let envelope = SavedEnvelope {
            storage_version: 1,
            saved_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(AppError::io)?
                .as_millis()
                .to_string(),
            document_sha256: format!("{:x}", Sha256::digest(&bytes)),
            document: document.clone(),
        };
        atomic_json(&self.project_file(document.project_id), &envelope)
    }
    fn read(&self, id: Uuid) -> AppResult<SavedEnvelope> {
        let file = self.project_file(id);
        if fs::metadata(&file).map_err(AppError::io)?.len() > MAX_DOCUMENT_BYTES {
            return Err(AppError::new(
                "project-corrupt",
                "工程超过 8 MiB 元数据上限。",
            ));
        }
        let envelope: SavedEnvelope =
            serde_json::from_slice(&fs::read(file).map_err(AppError::io)?)
                .map_err(|_| AppError::new("project-corrupt", "工程文件损坏或格式不受支持。"))?;
        if envelope.storage_version != 1 {
            return Err(AppError::new(
                "schema-unsupported",
                "草稿存储版本不受支持。",
            ));
        }
        envelope.document.validate()?;
        let bytes = serde_json::to_vec(&envelope.document).map_err(AppError::io)?;
        if envelope.document.project_id != id
            || format!("{:x}", Sha256::digest(&bytes)) != envelope.document_sha256
        {
            return Err(AppError::new("project-corrupt", "工程完整性校验失败。"));
        }
        Ok(envelope)
    }
    pub fn open(&self, id: Uuid) -> AppResult<ProjectDocument> {
        let document = self.read(id)?.document;
        for asset in &document.editor.assets {
            self.image_bytes(&asset.id)?;
        }
        Ok(document)
    }
    fn library_preferences(&self) -> AppResult<LibraryPreferences> {
        let file = self.root.join("library.json");
        match fs::metadata(&file) {
            Ok(metadata) if metadata.len() > MAX_DOCUMENT_BYTES => {
                return Err(AppError::new(
                    "library-corrupt",
                    "设计库设置超过 8 MiB 上限。",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LibraryPreferences::default());
            }
            Err(error) => return Err(AppError::io(error)),
        }
        serde_json::from_slice(&fs::read(file).map_err(AppError::io)?)
            .map_err(|_| AppError::new("library-corrupt", "设计库设置文件损坏。"))
    }
    pub fn pin_project(&self, id: Uuid, pinned: bool) -> AppResult<()> {
        self.read(id)?;
        let mut preferences = self.library_preferences()?;
        // Deleted ids are inert; prune them when preferences next change.
        preferences
            .pinned_project_ids
            .retain(|id| self.project_file(*id).is_file());
        if pinned {
            preferences.pinned_project_ids.insert(id);
        } else {
            preferences.pinned_project_ids.remove(&id);
        }
        atomic_json(&self.root.join("library.json"), &preferences)
    }
    pub fn delete_project(&self, id: Uuid) -> AppResult<()> {
        let source = self.project_file(id);
        let metadata = fs::metadata(&source).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                AppError::new("project-missing", "设计已不存在，请刷新设计库。")
            } else {
                AppError::io(error)
            }
        })?;
        if !metadata.is_file() {
            return Err(AppError::new("project-corrupt", "工程路径不是文件。"));
        }
        let trash = self.root.join("trash").join("projects");
        fs::create_dir_all(&trash).map_err(AppError::io)?;
        // One rename removes the saved design from the library without deleting
        // its contents or any immutable image blobs shared with other designs.
        let destination = trash.join(format!("{id}-{}.json", Uuid::new_v4()));
        fs::rename(source, destination).map_err(AppError::io)
    }
    pub fn duplicate_project(&self, id: Uuid) -> AppResult<ProjectDocument> {
        // Copy the last saved document, independent of any active dirty session.
        let mut document = self.open(id)?;
        let (projects, _) = self.list_tolerant()?;
        let names: BTreeSet<_> = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect();
        let base = document.name.clone();
        let mut number = 1;
        loop {
            let suffix = if number == 1 {
                " 副本".to_string()
            } else {
                format!(" 副本 {number}")
            };
            let prefix: String = base.chars().take(100 - suffix.chars().count()).collect();
            let name = format!("{}{suffix}", prefix.trim_end());
            if !names.contains(name.as_str()) {
                document.name = name;
                break;
            }
            number += 1;
        }
        document.project_id = Uuid::new_v4();
        document.content_id = Uuid::new_v4();
        document.revision = "0".into();
        self.save(&document)?;
        Ok(document)
    }
    pub fn import_image(
        &self,
        bytes: &[u8],
        name: &str,
    ) -> AppResult<itasha_contracts::ImageAsset> {
        use image::{ImageFormat, ImageReader};
        let bad = || {
            AppError::new(
                "image-invalid",
                "仅支持 PNG/JPEG/WebP；文件最多 20 MiB，最长边 8192，像素总量最多 1600 万。",
            )
        };
        if bytes.len() > 20 * 1024 * 1024 {
            return Err(bad());
        }
        let reader = ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| bad())?;
        if !matches!(
            reader.format(),
            Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
        ) {
            return Err(bad());
        }
        let (width, height) = reader.into_dimensions().map_err(|_| bad())?;
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || u64::from(width) * u64::from(height) > 16_777_216
        {
            return Err(bad());
        }
        let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| bad())?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let decoded = reader.decode().map_err(|_| bad())?.into_rgba8();
        let mut png = std::io::Cursor::new(vec![]);
        decoded
            .write_to(&mut png, ImageFormat::Png)
            .map_err(AppError::io)?;
        let png = png.into_inner();
        if png.len() > 64 * 1024 * 1024 {
            return Err(bad());
        }
        let id = format!("{:x}", Sha256::digest(&png));
        let directory = self.root.join("blobs");
        fs::create_dir_all(&directory).map_err(AppError::io)?;
        let path = directory.join(format!("{id}.png"));
        if !path.is_file() {
            let mut temp = tempfile::NamedTempFile::new_in(&directory).map_err(AppError::io)?;
            temp.write_all(&png).map_err(AppError::io)?;
            temp.as_file().sync_all().map_err(AppError::io)?;
            temp.persist(&path).map_err(AppError::io)?;
        } else {
            self.image_bytes(&id)?;
        }
        let name: String = name.chars().filter(|c| !c.is_control()).take(100).collect();
        Ok(itasha_contracts::ImageAsset {
            id,
            name: if name.is_empty() {
                "图片素材".into()
            } else {
                name
            },
            width,
            height,
        })
    }
    pub fn image_bytes(&self, id: &str) -> AppResult<Vec<u8>> {
        if !itasha_contracts::valid_hash(id) {
            return Err(AppError::new("asset-invalid", "素材标识无效。"));
        }
        let path = self.root.join("blobs").join(format!("{id}.png"));
        if fs::metadata(&path).map_err(AppError::io)?.len() > 64 * 1024 * 1024 {
            return Err(AppError::new("asset-invalid", "素材文件超出预算。"));
        }
        let bytes = fs::read(path).map_err(AppError::io)?;
        if format!("{:x}", Sha256::digest(&bytes)) != id {
            return Err(AppError::new("asset-corrupt", "素材完整性校验失败。"));
        }
        Ok(bytes)
    }
    pub fn list(&self) -> AppResult<Vec<ProjectSummary>> {
        let preferences = self.library_preferences()?;
        let mut items = vec![];
        for entry in fs::read_dir(self.root.join("projects")).map_err(AppError::io)? {
            let file = entry.map_err(AppError::io)?.path();
            let Some(id) = file
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| Uuid::parse_str(s).ok())
            else {
                continue;
            };
            if file.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let envelope = self.read(id)?;
            items.push(ProjectSummary {
                project_id: id,
                name: envelope.document.name,
                revision: envelope.document.revision,
                content_id: envelope.document.content_id,
                saved_at_unix_ms: envelope.saved_at_unix_ms,
                vehicle_id: envelope.document.vehicle_id,
                pinned: preferences.pinned_project_ids.contains(&id),
            });
        }
        sort_projects(&mut items);
        Ok(items)
    }

    /// Lists usable projects while isolating unsupported or damaged files.
    /// A single legacy project must not prevent creating a new project.
    pub fn list_tolerant(&self) -> AppResult<(Vec<ProjectSummary>, Vec<String>)> {
        let mut items = vec![];
        let mut warnings = vec![];
        let preferences = match self.library_preferences() {
            Ok(preferences) => preferences,
            Err(error) => {
                warnings.push(format!("设计库设置：{}", error.message));
                LibraryPreferences::default()
            }
        };
        for entry in fs::read_dir(self.root.join("projects")).map_err(AppError::io)? {
            let file = entry.map_err(AppError::io)?.path();
            let Some(id) = file
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| Uuid::parse_str(s).ok())
            else {
                continue;
            };
            if file.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            match self.read(id) {
                Ok(envelope) => items.push(ProjectSummary {
                    project_id: id,
                    name: envelope.document.name,
                    revision: envelope.document.revision,
                    content_id: envelope.document.content_id,
                    saved_at_unix_ms: envelope.saved_at_unix_ms,
                    vehicle_id: envelope.document.vehicle_id,
                    pinned: preferences.pinned_project_ids.contains(&id),
                }),
                Err(error) => warnings.push(format!(
                    "{}：{}",
                    file.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("未知工程"),
                    error.message
                )),
            }
        }
        sort_projects(&mut items);
        Ok((items, warnings))
    }
    pub fn settings(&self) -> AppResult<Settings> {
        let file = self.root.join("settings.json");
        match fs::read(file) {
            Ok(bytes) => {
                let value: Settings = serde_json::from_slice(&bytes)
                    .map_err(|_| AppError::new("invalid-input", "设置文件损坏。"))?;
                value.validate()?;
                Ok(value)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(AppError::io(e)),
        }
    }
    pub fn write_settings(&self, value: &Settings) -> AppResult<()> {
        value.validate()?;
        for (path, writable) in [
            (&value.paths.models, false),
            (&value.paths.exports, true),
            (&value.paths.cache, true),
        ] {
            if path.is_empty() {
                continue;
            }
            let directory = Path::new(path);
            if !directory.is_absolute() || !directory.is_dir() {
                return Err(AppError::new(
                    "invalid-path",
                    "请选择已存在的绝对目录路径。",
                ));
            }
            fs::read_dir(directory).map_err(AppError::io)?;
            if writable {
                let probe = tempfile::NamedTempFile::new_in(directory).map_err(AppError::io)?;
                probe.close().map_err(AppError::io)?;
            }
        }
        atomic_json(&self.root.join("settings.json"), value)
    }
}

fn sort_projects(items: &mut [ProjectSummary]) {
    items.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| {
                b.saved_at_unix_ms
                    .parse::<u128>()
                    .unwrap_or_default()
                    .cmp(&a.saved_at_unix_ms.parse::<u128>().unwrap_or_default())
            })
            .then(a.project_id.cmp(&b.project_id))
    });
}

fn atomic_json(file: &Path, value: &impl Serialize) -> AppResult<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(AppError::io)?;
    if bytes.len() > MAX_DOCUMENT_BYTES as usize {
        return Err(AppError::new(
            "document-too-large",
            "元数据超过 8 MiB 上限。",
        ));
    }
    let parent = file
        .parent()
        .ok_or_else(|| AppError::new("io-error", "存储目录无效。"))?;
    let mut pending = tempfile::NamedTempFile::new_in(parent).map_err(AppError::io)?;
    pending.write_all(&bytes).map_err(AppError::io)?;
    pending.as_file().sync_all().map_err(AppError::io)?;
    // NamedTempFile::persist replaces the destination on Windows as well as Unix.
    // Atomic visibility is guaranteed by the implementation; power-loss durability is not claimed.
    pending.persist(file).map_err(AppError::io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn images_are_normalized_deduplicated_and_preserved_with_layers() {
        use itasha_contracts::*;
        use itasha_domain::{Session, vehicle_catalog};
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        let pixels = image::RgbaImage::from_pixel(16, 8, image::Rgba([250, 80, 30, 128]));
        let mut output = std::io::Cursor::new(vec![]);
        pixels
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        let a = repo.import_image(output.get_ref(), "test.png").unwrap();
        let b = repo
            .import_image(output.get_ref(), "test again.png")
            .unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(a.width, 16);
        assert!(repo.image_bytes("../escape").is_err());
        assert!(repo.import_image(b"not an image", "bad").is_err());
        let vehicle = vehicle_catalog().iter().find(|v| v.id == "su7").unwrap();
        let mut s = Session::new(
            ProjectDocument::configured("素材", vehicle.id.clone(), vehicle.defaults.clone())
                .unwrap(),
            false,
        );
        s.add_asset(a.clone()).unwrap();
        s.edit(EditOperation::Insert {
            fingerprint: "a".repeat(64),
            layer: Layer {
                id: Uuid::new_v4(),
                name: "图片".into(),
                visible: true,
                locked: false,
                opacity: 0.5,
                placement: Placement {
                    anchor: Some(itasha_contracts::SurfaceAnchor {
                        part_id: "su7.hood".into(),
                        region_id: "outer".into(),
                        source_triangle_id: "test:0:0:0".into(),
                        barycentric: [1., 0., 0.],
                        paint_surface_fingerprint: "a".repeat(64),
                    }),
                    cross_surface: false,
                    flip_x: false,
                    aspect_ratio_locked: false,
                    surface_id: "su7.hood".into(),
                    profile_version: 2,
                    x_mm: 250.,
                    y_mm: 300.,
                    width_mm: 200.,
                    height_mm: 100.,
                    rotation_deg: 15.,
                },
                source: LayerSource::Image {
                    asset_id: a.id.clone(),
                },
            },
        })
        .unwrap();
        repo.save(&s.document).unwrap();
        assert_eq!(repo.open(s.document.project_id).unwrap(), s.document);
        // Absent lock metadata keeps existing current-format document checksums stable.
        let encoded = serde_json::to_string(&s.document).unwrap();
        assert!(!encoded.contains("aspectRatioLocked"));
        s.document.editor.layers[0].placement.aspect_ratio_locked = true;
        repo.save(&s.document).unwrap();
        assert!(
            repo.open(s.document.project_id).unwrap().editor.layers[0]
                .placement
                .aspect_ratio_locked
        );
        let duplicate = repo.duplicate_project(s.document.project_id).unwrap();
        assert_eq!(duplicate.editor, s.document.editor);
        assert_eq!(duplicate.configuration, s.document.configuration);
        assert_eq!(repo.open(duplicate.project_id).unwrap(), duplicate);
        fs::write(
            dir.path().join("blobs").join(format!("{}.png", a.id)),
            b"tampered",
        )
        .unwrap();
        assert_eq!(
            repo.open(s.document.project_id).unwrap_err().code,
            "asset-corrupt"
        );
    }
    #[test]
    fn configured_project_and_current_draft_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        let vehicle = &itasha_domain::vehicle_catalog()[0];
        let document =
            ProjectDocument::configured("选配工程", vehicle.id.clone(), vehicle.defaults.clone())
                .unwrap();
        repo.save(&document).unwrap();
        assert_eq!(repo.open(document.project_id).unwrap(), document);
        assert_eq!(repo.list().unwrap()[0].vehicle_id, Some(vehicle.id.clone()));
        let draft = ProjectDocument::new("早期草稿").unwrap();
        let bytes = serde_json::to_vec(&draft).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("configuration"));
        repo.save(&draft).unwrap();
        assert_eq!(repo.open(draft.project_id).unwrap(), draft);
    }
    #[test]
    fn pinned_designs_survive_restart_without_modifying_saved_documents() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        let first = ProjectDocument::new("A").unwrap();
        let second = ProjectDocument::new("B").unwrap();
        repo.save(&first).unwrap();
        repo.save(&second).unwrap();
        // Different digit counts also verify numeric rather than lexical sorting.
        let mut first_saved = repo.read(first.project_id).unwrap();
        first_saved.saved_at_unix_ms = "9".into();
        atomic_json(&repo.project_file(first.project_id), &first_saved).unwrap();
        let mut second_saved = repo.read(second.project_id).unwrap();
        second_saved.saved_at_unix_ms = "10".into();
        atomic_json(&repo.project_file(second.project_id), &second_saved).unwrap();
        let original = fs::read(repo.project_file(first.project_id)).unwrap();
        assert_eq!(repo.list().unwrap()[0].project_id, second.project_id);
        repo.pin_project(first.project_id, true).unwrap();
        assert_eq!(
            fs::read(repo.project_file(first.project_id)).unwrap(),
            original
        );
        drop(repo);
        let repo = Repository::new(dir.path().into()).unwrap();
        let (projects, warnings) = repo.list_tolerant().unwrap();
        assert!(warnings.is_empty());
        assert_eq!(projects[0].project_id, first.project_id);
        assert!(projects[0].pinned);
        assert!(!projects[1].pinned);
        repo.pin_project(first.project_id, false).unwrap();
        assert_eq!(repo.list().unwrap()[0].project_id, second.project_id);
        assert!(repo.list().unwrap().iter().all(|p| !p.pinned));
        assert!(repo.pin_project(Uuid::new_v4(), true).is_err());
        assert!(repo.list().unwrap().iter().all(|p| !p.pinned));
    }
    #[test]
    fn duplicate_has_independent_identity_and_unique_bounded_name() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        let vehicle = &itasha_domain::vehicle_catalog()[0];
        let mut source = ProjectDocument::configured(
            &"车".repeat(100),
            vehicle.id.clone(),
            vehicle.defaults.clone(),
        )
        .unwrap();
        source.revision = "12".into();
        repo.save(&source).unwrap();
        repo.pin_project(source.project_id, true).unwrap();
        let original = fs::read(repo.project_file(source.project_id)).unwrap();
        let mut copy = repo.duplicate_project(source.project_id).unwrap();
        let another = repo.duplicate_project(source.project_id).unwrap();
        assert_ne!(copy.project_id, source.project_id);
        assert_ne!(copy.content_id, source.content_id);
        assert_ne!(copy.project_id, another.project_id);
        assert_ne!(copy.name, another.name);
        assert!(copy.name.ends_with(" 副本"));
        assert!(another.name.ends_with(" 副本 2"));
        assert!(copy.name.chars().count() <= 100);
        assert!(another.name.chars().count() <= 100);
        assert_eq!(copy.revision, "0");
        assert_eq!(copy.vehicle_id, source.vehicle_id);
        assert_eq!(copy.configuration, source.configuration);
        assert_eq!(copy.editor, source.editor);
        assert!(
            !repo
                .list()
                .unwrap()
                .iter()
                .find(|p| p.project_id == copy.project_id)
                .unwrap()
                .pinned
        );
        copy.name = "独立修改副本".into();
        repo.save(&copy).unwrap();
        assert_eq!(
            fs::read(repo.project_file(source.project_id)).unwrap(),
            original
        );
        assert_eq!(repo.open(source.project_id).unwrap(), source);
        let count = repo.list().unwrap().len();
        assert!(repo.duplicate_project(Uuid::new_v4()).is_err());
        assert_eq!(repo.list().unwrap().len(), count);
    }
    #[test]
    fn delete_archives_only_the_target_document_and_retains_shared_assets() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        let pixels = image::RgbaImage::from_pixel(2, 2, image::Rgba([250, 80, 30, 255]));
        let mut output = std::io::Cursor::new(vec![]);
        pixels
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        let asset = repo.import_image(output.get_ref(), "共有素材").unwrap();
        let vehicle = &itasha_domain::vehicle_catalog()[0];
        let mut source =
            ProjectDocument::configured("A", vehicle.id.clone(), vehicle.defaults.clone()).unwrap();
        source.editor.assets.push(asset.clone());
        repo.save(&source).unwrap();
        let remaining = repo.duplicate_project(source.project_id).unwrap();
        repo.pin_project(source.project_id, true).unwrap();
        let original = fs::read(repo.project_file(source.project_id)).unwrap();
        let other_bytes = fs::read(repo.project_file(remaining.project_id)).unwrap();
        repo.delete_project(source.project_id).unwrap();
        assert!(!repo.project_file(source.project_id).exists());
        let (projects, warnings) = repo.list_tolerant().unwrap();
        assert!(warnings.is_empty());
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].project_id, remaining.project_id);
        let trash: Vec<_> = fs::read_dir(dir.path().join("trash/projects"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(trash.len(), 1);
        assert_eq!(fs::read(&trash[0]).unwrap(), original);
        assert_eq!(
            fs::read(repo.project_file(remaining.project_id)).unwrap(),
            other_bytes
        );
        assert_eq!(repo.open(remaining.project_id).unwrap(), remaining);
        assert!(!repo.image_bytes(&asset.id).unwrap().is_empty());
        assert_eq!(
            repo.delete_project(source.project_id).unwrap_err().code,
            "project-missing"
        );
        assert_eq!(repo.list().unwrap().len(), 1);
    }
    #[test]
    fn damaged_library_preferences_do_not_hide_saved_designs() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        let document = ProjectDocument::new("A").unwrap();
        repo.save(&document).unwrap();
        fs::write(dir.path().join("library.json"), b"bad json").unwrap();
        let (projects, warnings) = repo.list_tolerant().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].project_id, document.project_id);
        assert!(!projects[0].pinned);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("设计库设置"));
        assert!(repo.pin_project(document.project_id, true).is_err());
        assert_eq!(
            fs::read(dir.path().join("library.json")).unwrap(),
            b"bad json"
        );
    }
    #[test]
    fn unsupported_project_does_not_block_listing_current_projects() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        let mut legacy = ProjectDocument::new("旧版本工程").unwrap();
        legacy.document_version = 2;
        let bytes = serde_json::to_vec(&legacy).unwrap();
        let envelope = SavedEnvelope {
            storage_version: 1,
            saved_at_unix_ms: "1".into(),
            document_sha256: format!("{:x}", Sha256::digest(&bytes)),
            document: legacy.clone(),
        };
        atomic_json(&repo.project_file(legacy.project_id), &envelope).unwrap();

        let current = ProjectDocument::new("新版本工程").unwrap();
        repo.save(&current).unwrap();
        let (projects, warnings) = repo.list_tolerant().unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].project_id, current.project_id);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("不支持此工程版本"));
        assert!(repo.project_file(legacy.project_id).is_file());
    }
    #[test]
    fn first_shell_settings_load_with_new_defaults_and_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        fs::write(
            dir.path().join("settings.json"),
            r#"{"schemaVersion":1,"layout":"both","autosave":false}"#,
        )
        .unwrap();
        let mut value = repo.settings().unwrap();
        assert_eq!(value.workspace.direction, "vertical");
        value.workspace.direction = "horizontal".into();
        value.shortcuts.save = "Ctrl+Shift+S".into();
        value.paths.exports = dir.path().to_string_lossy().into_owned();
        repo.write_settings(&value).unwrap();
        assert_eq!(repo.settings().unwrap(), value);
    }
    #[test]
    fn missing_path_and_invalid_settings_do_not_overwrite_previous_file() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        let mut value = Settings::default();
        repo.write_settings(&value).unwrap();
        value.paths.cache = dir
            .path()
            .join("does-not-exist")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            repo.write_settings(&value).unwrap_err().code,
            "invalid-path"
        );
        assert_eq!(repo.settings().unwrap(), Settings::default());
    }
    #[test]
    fn exclusive_workspace_lock_is_released_when_repository_drops() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        assert_eq!(
            Repository::new(dir.path().into()).err().unwrap().code,
            "workspace-in-use"
        );
        let clone = repo.clone();
        drop(repo);
        assert!(Repository::new(dir.path().into()).is_err());
        drop(clone);
        assert!(Repository::new(dir.path().into()).is_ok());
    }
    #[test]
    fn save_overwrites_and_reopens_with_same_id() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        let mut doc = ProjectDocument::new("第一版").unwrap();
        repo.save(&doc).unwrap();
        doc.name = "覆盖保存".into();
        repo.save(&doc).unwrap();
        assert_eq!(repo.open(doc.project_id).unwrap(), doc);
        assert_eq!(repo.list().unwrap().len(), 1);
    }
    #[test]
    fn corrupted_content_fails_hash_check() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        let doc = ProjectDocument::new("原名").unwrap();
        repo.save(&doc).unwrap();
        let p = repo.project_file(doc.project_id);
        let text = fs::read_to_string(&p).unwrap().replace("原名", "篡改");
        fs::write(p, text).unwrap();
        assert_eq!(
            repo.open(doc.project_id).unwrap_err().code,
            "project-corrupt"
        );
    }
    #[test]
    fn invalid_settings_leave_saved_settings_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::new(dir.path().into()).unwrap();
        let mut value = Settings::default();
        repo.write_settings(&value).unwrap();
        value.layout = "invalid".into();
        assert!(repo.write_settings(&value).is_err());
        assert_eq!(repo.settings().unwrap(), Settings::default());
    }
}
