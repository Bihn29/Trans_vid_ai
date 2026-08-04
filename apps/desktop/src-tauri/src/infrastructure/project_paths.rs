use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
};

use uuid::Uuid;

use crate::domain::CoreError;

const PROJECT_DIRECTORIES: &[&str] = &[
    "source",
    "proxy",
    "audio/original",
    "audio/vocals",
    "audio/background",
    "audio/music",
    "audio/tts",
    "audio/mixed",
    "subtitles",
    "metadata",
    "previews",
    "renders",
    "logs",
    "temp",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRelativePath(String);

impl ProjectRelativePath {
    pub fn parse(value: impl Into<String>) -> Result<Self, CoreError> {
        let value = value.into();
        let path = Path::new(&value);
        if value.is_empty()
            || value.len() > 240
            || value.contains('\\')
            || value.contains(':')
            || path.is_absolute()
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-')
            })
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::CurDir
                        | Component::ParentDir
                        | Component::RootDir
                        | Component::Prefix(_)
                )
            })
        {
            return Err(CoreError::UnsafePath);
        }

        for component in value.split('/') {
            if component.is_empty()
                || component.ends_with('.')
                || component.ends_with(' ')
                || is_reserved_windows_name(component)
            {
                return Err(CoreError::UnsafePath);
            }
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn to_path_buf(&self) -> PathBuf {
        self.0.split('/').collect()
    }
}

#[derive(Debug, Clone)]
pub struct ProjectLayout {
    root: PathBuf,
}

impl ProjectLayout {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, CoreError> {
        fs::create_dir_all(root.as_ref())?;
        let root = root.as_ref().canonicalize()?;
        if !root.is_dir() {
            return Err(CoreError::UnsafePath);
        }
        fs::create_dir_all(root.join(".trash"))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn create_project(&self, id: Uuid) -> Result<PathBuf, CoreError> {
        let destination = self.root.join(id.to_string());
        if destination.exists() {
            return Err(CoreError::Conflict("project directory"));
        }
        let temporary = self.root.join(format!(".creating-{id}"));
        if temporary.exists() {
            return Err(CoreError::Conflict("project staging directory"));
        }

        let result = (|| {
            fs::create_dir(&temporary)?;
            for directory in PROJECT_DIRECTORIES {
                fs::create_dir_all(temporary.join(directory))?;
            }
            fs::rename(&temporary, &destination)?;
            self.project_root(id)
        })();

        if result.is_err() && temporary.exists() {
            let _ = fs::remove_dir_all(&temporary);
        }
        result
    }

    pub fn project_root(&self, id: Uuid) -> Result<PathBuf, CoreError> {
        let path = self.root.join(id.to_string());
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CoreError::UnsafePath);
        }
        let canonical = path.canonicalize()?;
        if canonical.parent() != Some(self.root.as_path())
            || canonical.file_name() != Some(OsStr::new(&id.to_string()))
        {
            return Err(CoreError::UnsafePath);
        }
        Ok(canonical)
    }

    pub fn resolve_existing(
        &self,
        project_id: Uuid,
        relative: &ProjectRelativePath,
    ) -> Result<PathBuf, CoreError> {
        let project_root = self.project_root(project_id)?;
        let candidate = project_root.join(relative.to_path_buf());
        let canonical = candidate.canonicalize()?;
        if canonical == project_root || !canonical.starts_with(&project_root) {
            return Err(CoreError::UnsafePath);
        }
        Ok(canonical)
    }

    pub fn prepare_output(
        &self,
        project_id: Uuid,
        relative: &ProjectRelativePath,
    ) -> Result<PathBuf, CoreError> {
        let project_root = self.project_root(project_id)?;
        let candidate = project_root.join(relative.to_path_buf());
        let parent = candidate.parent().ok_or(CoreError::UnsafePath)?;
        let canonical_parent = parent.canonicalize()?;
        if !canonical_parent.starts_with(&project_root) {
            return Err(CoreError::UnsafePath);
        }
        if candidate.exists() {
            let canonical = candidate.canonicalize()?;
            if !canonical.starts_with(&project_root) {
                return Err(CoreError::UnsafePath);
            }
        }
        let filename = candidate.file_name().ok_or(CoreError::UnsafePath)?;
        Ok(canonical_parent.join(filename))
    }

    pub fn move_to_trash(&self, project_id: Uuid) -> Result<PathBuf, CoreError> {
        let project_root = self.project_root(project_id)?;
        let trash_root = self.root.join(".trash").canonicalize()?;
        if trash_root.parent() != Some(self.root.as_path()) {
            return Err(CoreError::UnsafePath);
        }
        let destination = trash_root.join(format!("{project_id}-{}", Uuid::new_v4()));
        fs::rename(project_root, &destination)?;
        Ok(destination)
    }

    pub fn restore_from_trash(
        &self,
        project_id: Uuid,
        trashed_path: &Path,
    ) -> Result<(), CoreError> {
        let trash_root = self.root.join(".trash").canonicalize()?;
        let canonical = trashed_path.canonicalize()?;
        if !canonical.starts_with(&trash_root) {
            return Err(CoreError::UnsafePath);
        }
        fs::rename(canonical, self.root.join(project_id.to_string()))?;
        Ok(())
    }

    pub fn discard_created_project(&self, project_id: Uuid) -> Result<(), CoreError> {
        let project_root = self.project_root(project_id)?;
        fs::remove_dir_all(project_root)?;
        Ok(())
    }
}

fn is_reserved_windows_name(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_reject_traversal_and_windows_aliases() {
        assert!(ProjectRelativePath::parse("metadata/result.json").is_ok());
        for unsafe_path in [
            "../outside",
            "metadata/../outside",
            "C:/outside",
            "/outside",
            "metadata\\outside",
            "CON/file.txt",
            "renders/output.",
        ] {
            assert!(
                ProjectRelativePath::parse(unsafe_path).is_err(),
                "{unsafe_path}"
            );
        }
    }

    #[test]
    fn project_layout_is_complete_and_isolated() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let layout = ProjectLayout::new(temporary.path().join("projects")).expect("layout");
        let project_id = Uuid::new_v4();
        let root = layout.create_project(project_id).expect("create project");

        for directory in PROJECT_DIRECTORIES {
            assert!(root.join(directory).is_dir(), "{directory}");
        }
        assert!(layout.project_root(project_id).is_ok());
        assert!(layout.project_root(Uuid::new_v4()).is_err());
    }
}
