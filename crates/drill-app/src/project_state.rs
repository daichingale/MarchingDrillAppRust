use super::i18n::StatusMessage;
use drill_core::Document;
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg};
use drill_project::container::{
    self, AssetEntry, AssetId, AssetKind, AssetLocation, LoadedProject, SaveProject,
};
use drill_project::recovery::{self, RecoveryCandidate, Session};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Why a save or open stopped. `code` is a stable `JobErrorCode` name, or
/// `Cancelled`. The UI turns it into a sentence; the file format is untouched.
pub(crate) struct ProjectFailure {
    pub saving: bool,
    pub code: String,
}

impl std::fmt::Display for ProjectFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.code)
    }
}

pub(crate) enum ProjectEvent {
    Saved(PathBuf),
    Loaded {
        path: PathBuf,
        project: Box<LoadedProject>,
    },
    Failed(ProjectFailure),
}

struct Startup {
    session: Session,
    recoveries: Vec<RecoveryCandidate>,
    crashes: Vec<drill_project::crash::CrashReportFile>,
}

pub(crate) struct ProjectState {
    save: Option<Job<PathBuf>>,
    load: Option<Job<(PathBuf, LoadedProject)>>,
    startup: Option<Job<Startup>>,
    session: Option<Session>,
    pub recoveries: Vec<RecoveryCandidate>,
    pub crashes: Vec<drill_project::crash::CrashReportFile>,
    last_heartbeat: Instant,
    pub status: StatusMessage,
}

impl ProjectState {
    pub fn new() -> Self {
        let app_data = app_data_dir();
        let startup = Job::spawn_typed(JobKind::AutoSave, move |_| {
            let recoveries = recovery::scan_recoverable(&app_data, Duration::from_secs(30));
            let crashes = drill_project::crash::scan_reports(&app_data, 3);
            let session = Session::open_new(&app_data, env!("CARGO_PKG_VERSION"))
                .map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            Ok(Startup {
                session,
                recoveries,
                crashes,
            })
        });
        Self {
            save: None,
            load: None,
            startup: Some(startup),
            session: None,
            recoveries: Vec::new(),
            crashes: Vec::new(),
            last_heartbeat: Instant::now(),
            status: StatusMessage::new("project-status.001"),
        }
    }

    pub fn save_project(
        &mut self,
        path: PathBuf,
        document: Document,
        embed_audio: bool,
        underlay_bytes: Option<Vec<u8>>,
    ) {
        self.status = StatusMessage::new("project-status.002");
        self.save = Some(Job::spawn_typed(JobKind::Save, move |_| {
            let mut assets = Vec::new();
            let mut embedded = BTreeMap::new();
            if let Some(track) = &document.audio {
                let source = PathBuf::from(&track.path);
                let bytes =
                    std::fs::read(&source).map_err(|_| JobFailure::new(JobErrorCode::Io))?;
                let id = AssetId(1);
                let original_name = source
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let location = if embed_audio {
                    embedded.insert(id, bytes.clone());
                    AssetLocation::Embedded {
                        entry: format!("assets/{original_name}"),
                    }
                } else {
                    let relative = path
                        .parent()
                        .and_then(|base| source.strip_prefix(base).ok())
                        .map(|value| value.to_string_lossy().replace('\\', "/"));
                    AssetLocation::External {
                        relative,
                        absolute_hint: Some(source.to_string_lossy().into_owned()),
                    }
                };
                assets.push(AssetEntry {
                    id,
                    kind: AssetKind::Audio,
                    location,
                    original_name,
                    byte_len: bytes.len() as u64,
                    blake3_hex: blake3::hash(&bytes).to_hex().to_string(),
                });
            }
            if let Some(underlay) = &document.underlay {
                let id = AssetId(2);
                let location = if let Some(bytes) = underlay_bytes {
                    if blake3::hash(&bytes).to_hex().to_string() != underlay.content_hash {
                        return Err(JobFailure::new(JobErrorCode::Validation));
                    }
                    embedded.insert(id, bytes);
                    AssetLocation::Embedded {
                        entry: format!("assets/{}.image", underlay.content_hash),
                    }
                } else {
                    AssetLocation::External {
                        relative: None,
                        absolute_hint: underlay.external_path.clone(),
                    }
                };
                assets.push(AssetEntry {
                    id,
                    kind: AssetKind::Image,
                    location,
                    original_name: underlay.original_name.clone(),
                    byte_len: underlay.byte_len,
                    blake3_hex: underlay.content_hash.clone(),
                });
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string();
            container::save(
                &path,
                &SaveProject {
                    document: &document,
                    app_version: env!("CARGO_PKG_VERSION"),
                    created_utc: &now,
                    modified_utc: &now,
                    embedded: &embedded,
                    assets: &assets,
                },
            )
            .map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            Ok(path)
        }));
    }

    pub fn save_legacy_json(&mut self, path: PathBuf, document: Document) {
        self.status = StatusMessage::new("project-status.003");
        self.save = Some(Job::spawn_typed(JobKind::Save, move |_| {
            let json = document
                .to_json()
                .map_err(|_| JobFailure::new(JobErrorCode::Validation))?;
            let backup = path.with_extension("backup.drill.json");
            drill_project::atomic_write(
                &path,
                json.as_bytes(),
                path.exists().then_some(backup.as_path()),
            )
            .map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            Ok(path)
        }));
    }

    pub fn load_legacy_json(&mut self, path: PathBuf) {
        self.status = StatusMessage::new("project-status.004");
        self.load = Some(Job::spawn_typed(JobKind::Save, move |_| {
            let json =
                std::fs::read_to_string(&path).map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            let document =
                Document::from_json(&json).map_err(|_| JobFailure::new(JobErrorCode::Decode))?;
            Ok((path, legacy_project(document)))
        }));
    }

    pub fn load_project(&mut self, path: PathBuf) {
        self.status = StatusMessage::new("project-status.005");
        self.load = Some(Job::spawn_typed(JobKind::Save, move |_| {
            let loaded =
                container::load(&path).map_err(|_| JobFailure::new(JobErrorCode::Decode))?;
            Ok((path, loaded))
        }));
    }

    pub fn load_recovery(&mut self, index: usize) {
        let Some(candidate) = self.recoveries.get(index) else {
            return;
        };
        let path = candidate.autosave.clone();
        self.load = Some(Job::spawn_typed(JobKind::Save, move |_| {
            let json =
                std::fs::read_to_string(&path).map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            let document =
                Document::from_json(&json).map_err(|_| JobFailure::new(JobErrorCode::Decode))?;
            let project = LoadedProject {
                document,
                manifest: drill_project::container::Manifest {
                    container_version: 1,
                    document_schema_version: drill_core::SCHEMA_VERSION,
                    document_entry: "document.json".into(),
                    app_version: "recovery".into(),
                    created_utc: String::new(),
                    modified_utc: String::new(),
                    summary: Default::default(),
                    assets: Vec::new(),
                },
                embedded: BTreeMap::new(),
                warnings: Vec::new(),
            };
            Ok((path, project))
        }));
    }

    pub fn ignore_recovery(&mut self, index: usize) {
        if index < self.recoveries.len() {
            self.recoveries.remove(index);
        }
    }
    pub fn busy(&self) -> bool {
        self.save.is_some() || self.load.is_some() || self.startup.is_some()
    }

    /// Unlike `busy`, this answers only whether an explicit save is still
    /// pending.  Exit protection must not mistake a heartbeat or a load for a
    /// completed write.
    pub fn is_saving(&self) -> bool {
        self.save.is_some()
    }

    pub fn poll(&mut self) -> Option<ProjectEvent> {
        if let Some(message) = self.startup.as_mut().and_then(Job::poll) {
            self.startup = None;
            match message {
                JobMsg::Done(startup) => {
                    self.session = Some(startup.session);
                    self.recoveries = startup.recoveries;
                    self.crashes = startup.crashes;
                    self.status = StatusMessage::new("project-status.006");
                }
                JobMsg::Failed(error) => {
                    self.status = StatusMessage::new("project-status.007").arg(0, error)
                }
                JobMsg::Cancelled => {}
            }
        }
        if let Some(message) = self.save.as_mut().and_then(Job::poll) {
            self.save = None;
            return Some(match message {
                JobMsg::Done(path) => ProjectEvent::Saved(path),
                JobMsg::Failed(error) => ProjectEvent::Failed(ProjectFailure {
                    saving: true,
                    code: error.to_string(),
                }),
                JobMsg::Cancelled => ProjectEvent::Failed(ProjectFailure {
                    saving: true,
                    code: "Cancelled".into(),
                }),
            });
        }
        if let Some(message) = self.load.as_mut().and_then(Job::poll) {
            self.load = None;
            return Some(match message {
                JobMsg::Done((path, project)) => ProjectEvent::Loaded {
                    path,
                    project: Box::new(project),
                },
                JobMsg::Failed(error) => ProjectEvent::Failed(ProjectFailure {
                    saving: false,
                    code: error.to_string(),
                }),
                JobMsg::Cancelled => ProjectEvent::Failed(ProjectFailure {
                    saving: false,
                    code: "Cancelled".into(),
                }),
            });
        }
        None
    }

    pub fn autosave(&mut self, document: &Document, origin: Option<&Path>) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        let document = document.clone();
        let origin = origin.map(Path::to_path_buf);
        let recoveries = self.recoveries.clone();
        let crashes = self.crashes.clone();
        self.startup = Some(Job::spawn_typed(JobKind::AutoSave, move |_| {
            session
                .set_document_context(&document.title, origin.as_deref())
                .map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            session
                .write_autosave(&document)
                .map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            Ok(Startup {
                session,
                recoveries,
                crashes,
            })
        }));
        self.last_heartbeat = Instant::now();
    }

    pub fn heartbeat_if_due(&mut self) {
        if self.startup.is_some() || self.last_heartbeat.elapsed() < Duration::from_secs(10) {
            return;
        }
        let Some(mut session) = self.session.take() else {
            return;
        };
        let recoveries = self.recoveries.clone();
        let crashes = self.crashes.clone();
        self.startup = Some(Job::spawn_typed(JobKind::AutoSave, move |_| {
            session
                .heartbeat()
                .map_err(|_| JobFailure::new(JobErrorCode::Io))?;
            Ok(Startup {
                session,
                recoveries,
                crashes,
            })
        }));
        self.last_heartbeat = Instant::now();
    }
}

fn legacy_project(document: Document) -> LoadedProject {
    LoadedProject {
        document,
        manifest: drill_project::container::Manifest {
            container_version: 1,
            document_schema_version: drill_core::SCHEMA_VERSION,
            document_entry: "document.json".into(),
            app_version: "legacy-json".into(),
            created_utc: String::new(),
            modified_utc: String::new(),
            summary: Default::default(),
            assets: Vec::new(),
        },
        embedded: BTreeMap::new(),
        warnings: Vec::new(),
    }
}

impl Drop for ProjectState {
    fn drop(&mut self) {
        // Unwinding is not a clean shutdown. Leaving this session unmarked is
        // what makes its last atomic autosave discoverable on the next launch.
        if std::thread::panicking() {
            return;
        }
        if let Some(session) = self.session.take() {
            let _ = std::thread::Builder::new()
                .name("drill-session-close".into())
                .spawn(move || {
                    let _ = session.close_clean();
                });
        }
    }
}

pub(crate) fn app_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("DrillForge")
}
