use std::{fs, path::Path};

use serde_json::Value;
use uuid::Uuid;

use crate::{
    domain::{CoreError, NewProject, Project, ProjectUpdate},
    persistence::ProjectRepository,
};

use super::ProjectLayout;

#[derive(Clone)]
pub struct ProjectService {
    repository: ProjectRepository,
    layout: ProjectLayout,
}

impl ProjectService {
    pub fn new(repository: ProjectRepository, layout: ProjectLayout) -> Self {
        Self { repository, layout }
    }

    pub fn create(&self, new_project: &NewProject) -> Result<Project, CoreError> {
        new_project.validate()?;
        let id = Uuid::new_v4();
        self.layout.create_project(id)?;
        let result = self.repository.insert(id, new_project).and_then(|project| {
            self.write_snapshot(&project)?;
            Ok(project)
        });
        if result.is_err() {
            let _ = self.repository.delete(id);
            let _ = self.layout.discard_created_project(id);
        }
        result
    }

    pub fn get(&self, id: Uuid) -> Result<Project, CoreError> {
        self.repository.get(id)
    }

    pub fn list(&self) -> Result<Vec<Project>, CoreError> {
        self.repository.list()
    }

    pub fn update(&self, id: Uuid, update: &ProjectUpdate) -> Result<Project, CoreError> {
        let project = self.repository.update(id, update)?;
        self.write_snapshot(&project)?;
        Ok(project)
    }

    pub fn delete(&self, id: Uuid) -> Result<(), CoreError> {
        self.repository.get(id)?;
        let trashed_path = self.layout.move_to_trash(id)?;
        if let Err(error) = self.repository.delete(id) {
            let _ = self.layout.restore_from_trash(id, &trashed_path);
            return Err(error);
        }
        Ok(())
    }

    pub fn attach_source_asset(&self, id: Uuid, artifact_id: Uuid) -> Result<Project, CoreError> {
        if self.repository.get(id)?.source_asset_id.is_some() {
            return Err(CoreError::SourceAlreadySet);
        }
        let project = self.repository.update(
            id,
            &ProjectUpdate {
                source_asset_id: Some(Some(artifact_id)),
                ..ProjectUpdate::default()
            },
        )?;
        if let Err(error) = self.write_snapshot(&project) {
            let _ = self.repository.update(
                id,
                &ProjectUpdate {
                    source_asset_id: Some(None),
                    ..ProjectUpdate::default()
                },
            );
            return Err(error);
        }
        Ok(project)
    }

    pub fn layout(&self) -> &ProjectLayout {
        &self.layout
    }

    fn write_snapshot(&self, project: &Project) -> Result<(), CoreError> {
        let project_root = self.layout.project_root(project.id)?;
        let snapshot_path = project_root.join("project.json");
        let mut snapshot = serde_json::to_value(project)
            .map_err(|_| CoreError::InvalidInput("project snapshot"))?;
        let Value::Object(ref mut object) = snapshot else {
            return Err(CoreError::InvalidInput("project snapshot"));
        };
        object.insert("schema_version".into(), Value::from(1));
        let encoded = serde_json::to_vec_pretty(&snapshot)
            .map_err(|_| CoreError::InvalidInput("project snapshot"))?;
        write_derived_snapshot(&snapshot_path, &encoded)
    }
}

fn write_derived_snapshot(path: &Path, encoded: &[u8]) -> Result<(), CoreError> {
    let temporary = path.with_extension(format!("json.{}.tmp", Uuid::new_v4()));
    fs::write(&temporary, encoded)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}
