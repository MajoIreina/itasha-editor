use itasha_contracts::*;
use itasha_domain::{ProjectDocument, Session, vehicle_catalog};
use itasha_persistence::Repository;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Action {
    CreateConfigured {
        name: String,
        vehicle_id: String,
        configuration: VehicleConfiguration,
    },
    Create {
        name: String,
    },
    Open {
        id: Uuid,
    },
    DeleteProject {
        id: Uuid,
    },
    DuplicateProject {
        id: Uuid,
    },
    PinProject {
        id: Uuid,
        pinned: bool,
    },
    Rename {
        mutation: MutationRequest,
        name: String,
    },
    Edit {
        mutation: MutationRequest,
        operation: EditOperation,
    },
    Undo {
        mutation: MutationRequest,
    },
    Redo {
        mutation: MutationRequest,
    },
    Save {
        mutation: MutationRequest,
    },
    Discard {
        mutation: MutationRequest,
    },
    Settings {
        value: Settings,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub vehicles: &'static Vec<VehicleDefinition>,
    pub data_directory: String,
    pub project: Option<ProjectSnapshot>,
    pub projects: Vec<ProjectSummary>,
    pub project_warnings: Vec<String>,
    pub settings: Settings,
}

// The desktop host executes this serial application service on blocking workers.
// A save holds the service lock: edits cannot overtake disk writes in this increment.
pub struct Application {
    repository: Repository,
    session: Option<Session>,
    completed: VecDeque<(Uuid, String)>,
}

impl Application {
    pub fn remap_surface_mapping(
        &mut self,
        session: &SessionRef,
        from: &str,
        to: &str,
        remap: impl FnMut(&mut Layer) -> AppResult<()>,
    ) -> AppResult<bool> {
        self.session_check(session, None)?;
        self.session
            .as_mut()
            .expect("checked session")
            .remap_surface_mapping(from, to, remap)
    }

    pub fn rebind_surface_connections(
        &mut self,
        session: &SessionRef,
        from: &str,
        to: &str,
    ) -> AppResult<bool> {
        self.session_check(session, None)?;
        self.session
            .as_mut()
            .expect("checked session")
            .rebind_surface_connections(from, to)
    }
    pub fn canvas_layers(&self) -> Vec<Layer> {
        self.session
            .as_ref()
            .map(|s| s.document.editor.layers.clone())
            .unwrap_or_default()
    }
    pub fn canvas_stamp(&self) -> Option<(String, u64)> {
        self.session.as_ref().map(|s| {
            (
                s.epoch.to_string(),
                s.document.revision.parse().expect("validated revision"),
            )
        })
    }
    pub fn new(repository: Repository) -> Self {
        Self {
            repository,
            session: None,
            completed: VecDeque::new(),
        }
    }
    pub fn view(&self) -> AppResult<View> {
        let (projects, project_warnings) = self.repository.list_tolerant()?;
        Ok(View {
            vehicles: vehicle_catalog(),
            data_directory: self.repository.root(),
            project: self.session.as_ref().map(Session::snapshot),
            projects,
            project_warnings,
            settings: self.repository.settings()?,
        })
    }
    pub fn dirty(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.snapshot().dirty)
    }
    fn clean(&self) -> AppResult<()> {
        if self.dirty() {
            return Err(AppError::new(
                "unsaved-changes",
                "请先保存工程，或明确放弃未保存的修改。",
            ));
        }
        Ok(())
    }
    fn checked(&mut self, mutation: &MutationRequest) -> AppResult<&mut Session> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| AppError::new("no-session", "请先创建或打开工程。"))?;
        session.check(&mutation.session, &mutation.expected_revision)?;
        Ok(session)
    }
    pub fn execute(&mut self, request_id: Uuid, action: Action) -> AppResult<()> {
        let fingerprint = serde_json::to_string(&action).map_err(AppError::io)?;
        if let Some((_, prior)) = self.completed.iter().find(|(id, _)| *id == request_id) {
            return if *prior == fingerprint {
                Ok(())
            } else {
                Err(AppError::new(
                    "request-id-conflict",
                    "请求标识已用于其他操作。",
                ))
            };
        }
        match action {
            Action::CreateConfigured {
                name,
                vehicle_id,
                configuration,
            } => {
                self.clean()?;
                let document = ProjectDocument::configured(&name, vehicle_id, configuration)?;
                self.session = Some(Session::new(document, false));
            }
            Action::Create { name } => {
                self.clean()?;
                self.session = Some(Session::new(ProjectDocument::new(&name)?, false));
            }
            Action::Open { id } => {
                self.clean()?;
                let document = self.repository.open(id)?;
                self.session = Some(Session::new(document, true));
            }
            Action::DeleteProject { id } => {
                let is_current = self
                    .session
                    .as_ref()
                    .is_some_and(|s| s.document.project_id == id);
                if is_current {
                    self.clean()?;
                }
                self.repository.delete_project(id)?;
                if is_current {
                    self.session = None;
                }
            }
            Action::DuplicateProject { id } => {
                self.repository.duplicate_project(id)?;
            }
            Action::PinProject { id, pinned } => self.repository.pin_project(id, pinned)?,
            Action::Rename { mutation, name } => self.checked(&mutation)?.rename(&name)?,
            Action::Edit {
                mutation,
                operation,
            } => self.checked(&mutation)?.edit(operation)?,
            Action::Undo { mutation } => self.checked(&mutation)?.undo(false)?,
            Action::Redo { mutation } => self.checked(&mutation)?.undo(true)?,
            Action::Save { mutation } => {
                let document = self.checked(&mutation)?.document.clone();
                self.repository.save(&document)?;
                self.session.as_mut().unwrap().saved_content_id = Some(document.content_id);
            }
            Action::Discard { mutation } => {
                self.checked(&mutation)?;
                self.session = None;
            }
            Action::Settings { value } => self.repository.write_settings(&value)?,
        }
        self.completed.push_back((request_id, fingerprint));
        if self.completed.len() > 256 {
            self.completed.pop_front();
        }
        Ok(())
    }
    pub fn import_image(
        &mut self,
        mutation: &MutationRequest,
        bytes: &[u8],
        name: &str,
    ) -> AppResult<ImageAsset> {
        self.checked(mutation)?;
        let asset = self.repository.import_image(bytes, name)?;
        self.checked(mutation)?.add_asset(asset.clone())?;
        Ok(asset)
    }
    pub fn image_bytes(&self, session: &SessionRef, id: &str) -> AppResult<Vec<u8>> {
        let current = self
            .session
            .as_ref()
            .ok_or_else(|| AppError::new("no-session", "请先打开工程。"))?;
        current.check(session, &current.document.revision)?;
        if !current.document.editor.assets.iter().any(|a| a.id == id) {
            return Err(AppError::new("asset-missing", "素材不属于当前工程。"));
        }
        self.repository.image_bytes(id)
    }
    pub fn image_import_check(&mut self, mutation: &MutationRequest) -> AppResult<()> {
        self.checked(mutation)?;
        Ok(())
    }
    pub fn session_check(&self, session: &SessionRef, revision: Option<&str>) -> AppResult<()> {
        let s = self
            .session
            .as_ref()
            .ok_or_else(|| AppError::new("no-session", "请先打开工程。"))?;
        s.check(session, revision.unwrap_or(&s.document.revision))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_configured_creation_preserves_active_session_and_dirty_creation_is_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = Application::new(Repository::new(dir.path().into()).unwrap());
        let vehicle = &vehicle_catalog()[0];
        app.execute(
            Uuid::new_v4(),
            Action::CreateConfigured {
                name: "A".into(),
                vehicle_id: vehicle.id.clone(),
                configuration: vehicle.defaults.clone(),
            },
        )
        .unwrap();
        let before = app.view().unwrap().project;
        assert_eq!(
            app.execute(
                Uuid::new_v4(),
                Action::CreateConfigured {
                    name: "B".into(),
                    vehicle_id: vehicle.id.clone(),
                    configuration: vehicle.defaults.clone()
                }
            )
            .unwrap_err()
            .code,
            "unsaved-changes"
        );
        assert_eq!(before, app.view().unwrap().project);
        app.execute(
            Uuid::new_v4(),
            Action::Save {
                mutation: mutation(&app),
            },
        )
        .unwrap();
        let before = app.view().unwrap().project;
        let mut config = vehicle.defaults.clone();
        config.wheels = "missing".into();
        assert!(
            app.execute(
                Uuid::new_v4(),
                Action::CreateConfigured {
                    name: "C".into(),
                    vehicle_id: vehicle.id.clone(),
                    configuration: config
                }
            )
            .is_err()
        );
        assert_eq!(before, app.view().unwrap().project);
    }
    fn mutation(app: &Application) -> MutationRequest {
        let s = app.view().unwrap().project.unwrap();
        MutationRequest {
            request_id: Uuid::new_v4(),
            session: s.session,
            expected_revision: s.revision,
        }
    }
    #[test]
    fn retries_are_idempotent_and_payload_changes_fail() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = Application::new(Repository::new(dir.path().into()).unwrap());
        let id = Uuid::new_v4();
        let action = Action::Create { name: "A".into() };
        app.execute(id, action.clone()).unwrap();
        let before = app.view().unwrap().project;
        app.execute(id, action).unwrap();
        assert_eq!(before, app.view().unwrap().project);
        assert_eq!(
            app.execute(id, Action::Create { name: "B".into() })
                .unwrap_err()
                .code,
            "request-id-conflict"
        );
    }
    #[test]
    fn dirty_switch_and_failed_open_preserve_active_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = Application::new(Repository::new(dir.path().into()).unwrap());
        app.execute(Uuid::new_v4(), Action::Create { name: "A".into() })
            .unwrap();
        assert_eq!(
            app.execute(Uuid::new_v4(), Action::Open { id: Uuid::new_v4() })
                .unwrap_err()
                .code,
            "unsaved-changes"
        );
        app.execute(
            Uuid::new_v4(),
            Action::Save {
                mutation: mutation(&app),
            },
        )
        .unwrap();
        let before = app.view().unwrap().project;
        assert!(
            app.execute(Uuid::new_v4(), Action::Open { id: Uuid::new_v4() })
                .is_err()
        );
        assert_eq!(before, app.view().unwrap().project);
    }
    #[test]
    fn stale_revision_is_rejected_and_saved_undo_becomes_clean() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = Application::new(Repository::new(dir.path().into()).unwrap());
        app.execute(Uuid::new_v4(), Action::Create { name: "A".into() })
            .unwrap();
        app.execute(
            Uuid::new_v4(),
            Action::Save {
                mutation: mutation(&app),
            },
        )
        .unwrap();
        let old = mutation(&app);
        app.execute(
            Uuid::new_v4(),
            Action::Rename {
                mutation: old.clone(),
                name: "B".into(),
            },
        )
        .unwrap();
        assert_eq!(
            app.execute(Uuid::new_v4(), Action::Save { mutation: old })
                .unwrap_err()
                .code,
            "revision-conflict"
        );
        app.execute(
            Uuid::new_v4(),
            Action::Undo {
                mutation: mutation(&app),
            },
        )
        .unwrap();
        assert!(!app.dirty());
    }
    #[test]
    fn deleting_active_design_protects_dirty_changes_and_closes_clean_session() {
        let dir = tempfile::tempdir().unwrap();
        let repository = Repository::new(dir.path().into()).unwrap();
        let document = ProjectDocument::new("原工程").unwrap();
        repository.save(&document).unwrap();
        let mut app = Application::new(repository.clone());
        app.execute(
            Uuid::new_v4(),
            Action::Open {
                id: document.project_id,
            },
        )
        .unwrap();
        app.execute(
            Uuid::new_v4(),
            Action::Rename {
                mutation: mutation(&app),
                name: "未保存".into(),
            },
        )
        .unwrap();
        let before = app.view().unwrap().project;
        let request = Uuid::new_v4();
        assert_eq!(
            app.execute(
                request,
                Action::DeleteProject {
                    id: document.project_id
                }
            )
            .unwrap_err()
            .code,
            "unsaved-changes"
        );
        assert_eq!(app.view().unwrap().project, before);
        assert_eq!(repository.open(document.project_id).unwrap(), document);
        app.execute(
            Uuid::new_v4(),
            Action::Save {
                mutation: mutation(&app),
            },
        )
        .unwrap();
        app.execute(
            request,
            Action::DeleteProject {
                id: document.project_id,
            },
        )
        .unwrap();
        assert!(app.view().unwrap().project.is_none());
        assert!(app.view().unwrap().projects.is_empty());
        // Retried requests must not fail because the first delete moved the file.
        app.execute(
            request,
            Action::DeleteProject {
                id: document.project_id,
            },
        )
        .unwrap();
    }
    #[test]
    fn deleting_another_design_and_failed_delete_preserve_active_dirty_session() {
        let dir = tempfile::tempdir().unwrap();
        let repository = Repository::new(dir.path().into()).unwrap();
        let other = ProjectDocument::new("另一个工程").unwrap();
        repository.save(&other).unwrap();
        let mut app = Application::new(repository);
        app.execute(
            Uuid::new_v4(),
            Action::Create {
                name: "当前工程".into(),
            },
        )
        .unwrap();
        let before = app.view().unwrap().project;
        app.execute(
            Uuid::new_v4(),
            Action::DeleteProject {
                id: other.project_id,
            },
        )
        .unwrap();
        assert_eq!(app.view().unwrap().project, before);
        assert_eq!(
            app.execute(
                Uuid::new_v4(),
                Action::DeleteProject {
                    id: other.project_id
                }
            )
            .unwrap_err()
            .code,
            "project-missing"
        );
        assert_eq!(app.view().unwrap().project, before);
    }
    #[test]
    fn duplicate_uses_saved_content_once_and_pin_does_not_mutate_active_document() {
        let dir = tempfile::tempdir().unwrap();
        let repository = Repository::new(dir.path().into()).unwrap();
        let document = ProjectDocument::new("已保存名称").unwrap();
        repository.save(&document).unwrap();
        let mut app = Application::new(repository.clone());
        app.execute(
            Uuid::new_v4(),
            Action::Open {
                id: document.project_id,
            },
        )
        .unwrap();
        app.execute(
            Uuid::new_v4(),
            Action::Rename {
                mutation: mutation(&app),
                name: "未保存名称".into(),
            },
        )
        .unwrap();
        let before = app.view().unwrap().project;
        let request = Uuid::new_v4();
        app.execute(
            request,
            Action::DuplicateProject {
                id: document.project_id,
            },
        )
        .unwrap();
        app.execute(
            request,
            Action::DuplicateProject {
                id: document.project_id,
            },
        )
        .unwrap();
        app.execute(
            Uuid::new_v4(),
            Action::PinProject {
                id: document.project_id,
                pinned: true,
            },
        )
        .unwrap();
        let view = app.view().unwrap();
        assert_eq!(view.project, before);
        assert_eq!(view.projects.len(), 2);
        assert_eq!(view.projects[0].project_id, document.project_id);
        assert!(view.projects[0].pinned);
        assert_eq!(view.projects[1].name, "已保存名称 副本");
        assert!(!view.projects[1].pinned);
        assert_eq!(repository.open(document.project_id).unwrap(), document);
    }
}
