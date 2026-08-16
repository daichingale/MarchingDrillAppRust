//! Crash-safe session autosaves and recovery candidate discovery.

use crate::atomic_write;
use drill_core::Document;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const META_FILE: &str = "session.json";
const AUTOSAVE_FILE: &str = "autosave.drill.json";
const LOCK_FILE: &str = "active.lock";
const CLEAN_FILE: &str = "closed.clean";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: String,
    pub app_version: String,
    pub started_unix_ms: u64,
    pub heartbeat_unix_ms: u64,
    pub document_title: String,
    pub origin_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryCandidate {
    pub root: PathBuf,
    pub autosave: PathBuf,
    pub meta: SessionMeta,
    pub autosave_bytes: u64,
    pub stale_for: Duration,
}

#[derive(Debug)]
pub struct Session {
    root: PathBuf,
    lock: File,
    meta: SessionMeta,
}

impl Session {
    pub fn open_new(app_data: &Path, app_version: &str) -> io::Result<Self> {
        let sessions = app_data.join("recovery");
        fs::create_dir_all(&sessions)?;
        let now = unix_ms()?;
        for sequence in 0..1000u32 {
            let id = format!("{now}-{}-{sequence}", std::process::id());
            let root = sessions.join(&id);
            match fs::create_dir(&root) {
                Ok(()) => {
                    let lock = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(root.join(LOCK_FILE))?;
                    let meta = SessionMeta {
                        session_id: id,
                        app_version: app_version.into(),
                        started_unix_ms: now,
                        heartbeat_unix_ms: now,
                        document_title: String::new(),
                        origin_name: None,
                    };
                    let session = Self { root, lock, meta };
                    session.write_meta()?;
                    return Ok(session);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate recovery session",
        ))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn heartbeat(&mut self) -> io::Result<()> {
        self.meta.heartbeat_unix_ms = unix_ms()?;
        self.write_meta()
    }

    pub fn set_document_context(&mut self, title: &str, origin: Option<&Path>) -> io::Result<()> {
        self.meta.document_title = title.chars().take(512).collect();
        self.meta.origin_name = origin
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().chars().take(512).collect());
        self.write_meta()
    }

    pub fn write_autosave(&mut self, document: &Document) -> io::Result<PathBuf> {
        document
            .validate()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let json = document
            .to_json()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let path = self.root.join(AUTOSAVE_FILE);
        atomic_write(&path, json.as_bytes(), None)?;
        self.meta.document_title = document.title.clone();
        self.heartbeat()?;
        Ok(path)
    }

    pub fn close_clean(self) -> io::Result<()> {
        self.lock.sync_all()?;
        atomic_write(&self.root.join(CLEAN_FILE), b"clean\n", None)
    }

    fn write_meta(&self) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.meta)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        atomic_write(&self.root.join(META_FILE), &bytes, None)
    }
}

pub fn scan_recoverable(app_data: &Path, stale_after: Duration) -> Vec<RecoveryCandidate> {
    let sessions = app_data.join("recovery");
    let Ok(entries) = fs::read_dir(sessions) else {
        return Vec::new();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let mut candidates = Vec::new();
    for entry in entries.flatten().take(10_000) {
        let root = entry.path();
        if root.join(CLEAN_FILE).exists() {
            continue;
        }
        let autosave = root.join(AUTOSAVE_FILE);
        let Ok(metadata) = fs::metadata(&autosave) else {
            continue;
        };
        let Ok(meta_bytes) = fs::read(root.join(META_FILE)) else {
            continue;
        };
        if meta_bytes.len() > 1024 * 1024 {
            continue;
        }
        let Ok(meta) = serde_json::from_slice::<SessionMeta>(&meta_bytes) else {
            continue;
        };
        let heartbeat = Duration::from_millis(meta.heartbeat_unix_ms);
        let stale_for = now.saturating_sub(heartbeat);
        if stale_for < stale_after {
            continue;
        }
        candidates.push(RecoveryCandidate {
            root,
            autosave,
            meta,
            autosave_bytes: metadata.len(),
            stale_for,
        });
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.meta.heartbeat_unix_ms));
    candidates
}

fn unix_ms() -> io::Result<u64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    u64::try_from(duration.as_millis()).map_err(|_| io::Error::other("system time overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "drillforge-recovery-{name}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn unclean_session_with_autosave_is_discovered() {
        let root = root("discover");
        let mut session = Session::open_new(&root, "test").unwrap();
        session.write_autosave(&Document::demo(2, 2)).unwrap();
        drop(session);
        let candidates = scan_recoverable(&root, Duration::ZERO);
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            Document::from_json(&fs::read_to_string(&candidates[0].autosave).unwrap())
                .unwrap()
                .performers
                .len(),
            4
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clean_session_is_not_offered() {
        let root = root("clean");
        let mut session = Session::open_new(&root, "test").unwrap();
        session.write_autosave(&Document::demo(1, 1)).unwrap();
        session.close_clean().unwrap();
        assert!(scan_recoverable(&root, Duration::ZERO).is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
