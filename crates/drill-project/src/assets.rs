//! Bounded external asset resolution and explicit relinking.

use crate::container::{AssetEntry, AssetId, AssetLocation, MAX_ASSET_BYTES, ProjectError};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetSource {
    ProjectRelative,
    AbsoluteHint,
    ConventionFolder,
    RecentFolder,
    Relinked,
    FoundByHash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetState {
    Ready {
        path: PathBuf,
        source: AssetSource,
    },
    Missing {
        searched: Vec<PathBuf>,
    },
    Mismatch {
        path: PathBuf,
        found_blake3: String,
    },
    TooLarge {
        path: PathBuf,
        declared: u64,
        limit: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveLimits {
    pub recent_folders: usize,
    pub files_per_folder: usize,
    pub total_scan_bytes: u64,
}

impl Default for ResolveLimits {
    fn default() -> Self {
        Self {
            recent_folders: 16,
            files_per_folder: 2_000,
            total_scan_bytes: 4 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AssetTable {
    entries: BTreeMap<AssetId, (AssetEntry, AssetState)>,
}

impl AssetTable {
    pub fn from_external(
        entries: impl IntoIterator<Item = AssetEntry>,
        project_path: &Path,
        recent: &[PathBuf],
        limits: ResolveLimits,
    ) -> Self {
        let mut table = Self::default();
        for entry in entries {
            let state = resolve_one(&entry, project_path, recent, limits);
            table.entries.insert(entry.id, (entry, state));
        }
        table
    }

    pub fn entry(&self, id: AssetId) -> Option<&AssetEntry> {
        self.entries.get(&id).map(|value| &value.0)
    }
    pub fn state(&self, id: AssetId) -> Option<&AssetState> {
        self.entries.get(&id).map(|value| &value.1)
    }
    pub fn unresolved(&self) -> impl Iterator<Item = (AssetId, &AssetEntry, &AssetState)> {
        self.entries
            .iter()
            .filter(|(_, (_, state))| !matches!(state, AssetState::Ready { .. }))
            .map(|(&id, (entry, state))| (id, entry, state))
    }

    pub fn relink(&mut self, id: AssetId, path: &Path, force: bool) -> Result<(), ProjectError> {
        let (entry, state) = self
            .entries
            .get_mut(&id)
            .ok_or(ProjectError::MissingEntry("asset id"))?;
        let (bytes, hash) = bounded_read(path)?;
        if hash != entry.blake3_hex && !force {
            return Err(ProjectError::HashMismatch(entry.original_name.clone()));
        }
        if force {
            entry.blake3_hex = hash;
            entry.byte_len = bytes.len() as u64;
        }
        *state = AssetState::Ready {
            path: path.to_path_buf(),
            source: AssetSource::Relinked,
        };
        Ok(())
    }

    pub fn relink_folder(
        &mut self,
        folder: &Path,
        limits: ResolveLimits,
        cancel: &AtomicBool,
    ) -> usize {
        let Ok(read_dir) = fs::read_dir(folder) else {
            return 0;
        };
        let mut candidates = Vec::new();
        let mut scanned_bytes = 0u64;
        for item in read_dir.flatten().take(limits.files_per_folder) {
            if cancel.load(Ordering::Relaxed) {
                return 0;
            }
            let path = item.path();
            let Ok(meta) = item.metadata() else {
                continue;
            };
            if !meta.is_file()
                || meta.len() > MAX_ASSET_BYTES
                || scanned_bytes.saturating_add(meta.len()) > limits.total_scan_bytes
            {
                continue;
            }
            scanned_bytes = scanned_bytes.saturating_add(meta.len());
            if let Ok((_, hash)) = bounded_read(&path) {
                candidates.push((path, hash));
            }
        }
        let mut fixed = 0;
        for (entry, state) in self.entries.values_mut() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if matches!(state, AssetState::Ready { .. }) {
                continue;
            }
            if let Some((path, _)) = candidates
                .iter()
                .find(|(_, hash)| hash == &entry.blake3_hex)
            {
                *state = AssetState::Ready {
                    path: path.clone(),
                    source: AssetSource::FoundByHash,
                };
                fixed += 1;
            }
        }
        fixed
    }
}

fn resolve_one(
    entry: &AssetEntry,
    project_path: &Path,
    recent: &[PathBuf],
    limits: ResolveLimits,
) -> AssetState {
    let AssetLocation::External {
        relative,
        absolute_hint,
    } = &entry.location
    else {
        return AssetState::Missing {
            searched: Vec::new(),
        };
    };
    let project_dir = project_path.parent().unwrap_or(Path::new("."));
    let mut candidates = Vec::new();
    if let Some(relative) = relative
        && safe_relative(relative)
    {
        candidates.push((project_dir.join(relative), AssetSource::ProjectRelative));
    }
    if let Some(absolute) = absolute_hint {
        candidates.push((PathBuf::from(absolute), AssetSource::AbsoluteHint));
    }
    if let Some(stem) = project_path.file_stem() {
        candidates.push((
            project_dir
                .join(format!("{}_assets", stem.to_string_lossy()))
                .join(&entry.original_name),
            AssetSource::ConventionFolder,
        ));
    }
    for folder in recent.iter().take(limits.recent_folders) {
        candidates.push((folder.join(&entry.original_name), AssetSource::RecentFolder));
    }
    let mut searched = Vec::new();
    for (path, source) in candidates {
        searched.push(path.clone());
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.len() > MAX_ASSET_BYTES {
            return AssetState::TooLarge {
                path,
                declared: meta.len(),
                limit: MAX_ASSET_BYTES,
            };
        }
        if let Ok((_, hash)) = bounded_read(&path) {
            if hash == entry.blake3_hex {
                return AssetState::Ready { path, source };
            }
            return AssetState::Mismatch {
                path,
                found_blake3: hash,
            };
        }
    }
    AssetState::Missing { searched }
}

fn bounded_read(path: &Path) -> Result<(Vec<u8>, String), ProjectError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_ASSET_BYTES {
        return Err(ProjectError::LimitExceeded("asset"));
    }
    let bytes = fs::read(path)?;
    if bytes.len() as u64 != metadata.len() {
        return Err(ProjectError::Io("asset changed while reading".into()));
    }
    let hash = blake3::hash(&bytes).to_hex().to_string();
    Ok((bytes, hash))
}

fn safe_relative(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::AssetKind;
    static CANCEL: AtomicBool = AtomicBool::new(false);
    fn entry(id: u32, name: &str, bytes: &[u8]) -> AssetEntry {
        AssetEntry {
            id: AssetId(id),
            kind: AssetKind::Audio,
            location: AssetLocation::External {
                relative: Some(name.into()),
                absolute_hint: None,
            },
            original_name: name.into(),
            byte_len: bytes.len() as u64,
            blake3_hex: blake3::hash(bytes).to_hex().to_string(),
        }
    }
    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("drillforge-assets-{name}-{}", std::process::id()))
    }

    #[test]
    fn resolves_relative_and_reports_mismatch() {
        let root = root("resolve");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("ok.wav"), b"ok").unwrap();
        fs::write(root.join("bad.wav"), b"other").unwrap();
        let table = AssetTable::from_external(
            [entry(1, "ok.wav", b"ok"), entry(2, "bad.wav", b"wanted")],
            &root.join("show.drillproj"),
            &[],
            ResolveLimits::default(),
        );
        assert!(matches!(
            table.state(AssetId(1)),
            Some(AssetState::Ready {
                source: AssetSource::ProjectRelative,
                ..
            })
        ));
        assert!(matches!(
            table.state(AssetId(2)),
            Some(AssetState::Mismatch { .. })
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn folder_hash_scan_recovers_renamed_asset() {
        let root = root("scan");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("renamed.wav"), b"same").unwrap();
        let mut table = AssetTable::from_external(
            [entry(1, "missing.wav", b"same")],
            &root.join("show.drillproj"),
            &[],
            ResolveLimits::default(),
        );
        assert_eq!(
            table.relink_folder(&root, ResolveLimits::default(), &CANCEL),
            1
        );
        assert!(matches!(
            table.state(AssetId(1)),
            Some(AssetState::Ready {
                source: AssetSource::FoundByHash,
                ..
            })
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn traversal_relative_hint_is_never_used() {
        assert!(!safe_relative("../secret.wav"));
        assert!(!safe_relative("/absolute.wav"));
    }
}
