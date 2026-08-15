//! Small, deliberately boring persistence for the macOS-style Open Recent
//! surface. Paths are a convenience only: the document itself is never copied
//! here, and a missing entry is discarded rather than guessed or recreated.

use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 10;

#[derive(Default)]
pub(crate) struct RecentProjects {
    paths: Vec<PathBuf>,
    persisted: String,
}

impl RecentProjects {
    pub(crate) fn load() -> Self {
        let mut paths = preferences_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Vec<PathBuf>>(&bytes).ok())
            .unwrap_or_default();
        paths.retain(|path| path.is_file());
        paths.truncate(MAX_RECENT);
        let persisted = serde_json::to_string(&paths).unwrap_or_default();
        let state = Self {
            paths,
            persisted: String::new(),
        };
        // Persist pruning too: unavailable volumes and deleted files should not
        // keep reappearing in the menu on every launch.
        state.persist();
        Self { persisted, ..state }
    }

    pub(crate) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub(crate) fn remember(&mut self, path: PathBuf) {
        self.paths.retain(|existing| existing != &path);
        self.paths.insert(0, path);
        self.paths.truncate(MAX_RECENT);
        self.persist_if_changed();
    }

    pub(crate) fn prune_missing(&mut self) {
        self.paths.retain(|path| path.is_file());
        self.persist_if_changed();
    }

    pub(crate) fn remove(&mut self, path: &Path) {
        self.paths.retain(|candidate| candidate != path);
        self.persist_if_changed();
    }

    pub(crate) fn clear(&mut self) {
        self.paths.clear();
        self.persist_if_changed();
    }

    fn persist_if_changed(&mut self) {
        let json = serde_json::to_string(&self.paths).unwrap_or_default();
        if json != self.persisted {
            self.persist();
            self.persisted = json;
        }
    }

    fn persist(&self) {
        let Some(path) = preferences_path() else {
            return;
        };
        if path
            .parent()
            .is_none_or(|parent| std::fs::create_dir_all(parent).is_err())
        {
            return;
        }
        let bytes = serde_json::to_vec(&self.paths).unwrap_or_default();
        let _ = drill_project::atomic_write(&path, &bytes, None);
    }
}

fn preferences_path() -> Option<PathBuf> {
    Some(super::project_state::app_data_dir().join("recent-projects.json"))
}
