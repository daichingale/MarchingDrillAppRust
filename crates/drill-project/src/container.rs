//! Deterministic, bounded `.drillproj` ZIP container.

use crate::atomic_write;
use drill_core::{Document, DrillError, SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const MIMETYPE: &str = "application/x-drillforge-project";
pub const CONTAINER_VERSION: u16 = 1;
pub const MAX_CONTAINER_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 4096;
pub const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_ASSET_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectError {
    Io(String),
    Zip(String),
    Document(DrillError),
    InvalidManifest,
    UnsupportedContainer(u16),
    UnsafeEntry(String),
    LimitExceeded(&'static str),
    MissingEntry(&'static str),
    HashMismatch(String),
}

impl From<io::Error> for ProjectError {
    fn from(value: io::Error) -> Self {
        Self::Io(value.to_string())
    }
}
impl From<zip::result::ZipError> for ProjectError {
    fn from(value: zip::result::ZipError) -> Self {
        Self::Zip(value.to_string())
    }
}
impl From<DrillError> for ProjectError {
    fn from(value: DrillError) -> Self {
        Self::Document(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetKind {
    Audio,
    Image,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetLocation {
    Embedded {
        entry: String,
    },
    External {
        relative: Option<String>,
        absolute_hint: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetEntry {
    pub id: AssetId,
    pub kind: AssetKind,
    pub location: AssetLocation,
    pub original_name: String,
    pub byte_len: u64,
    pub blake3_hex: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSummary {
    pub title: String,
    pub performers: u32,
    pub sets: u32,
    pub total_counts: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub container_version: u16,
    pub document_schema_version: u16,
    pub document_entry: String,
    pub app_version: String,
    pub created_utc: String,
    pub modified_utc: String,
    pub summary: DocumentSummary,
    pub assets: Vec<AssetEntry>,
}

pub struct SaveProject<'a> {
    pub document: &'a Document,
    pub app_version: &'a str,
    pub created_utc: &'a str,
    pub modified_utc: &'a str,
    /// Embedded bytes keyed by manifest asset id. External entries are omitted.
    pub embedded: &'a BTreeMap<AssetId, Vec<u8>>,
    pub assets: &'a [AssetEntry],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadWarning {
    ExternalMissing { id: AssetId, name: String },
    ExternalMismatch { id: AssetId, path: PathBuf },
}

pub struct LoadedProject {
    pub document: Document,
    pub manifest: Manifest,
    pub embedded: BTreeMap<AssetId, Vec<u8>>,
    pub warnings: Vec<LoadWarning>,
}

pub fn save(path: &Path, request: &SaveProject<'_>) -> Result<(), ProjectError> {
    request.document.validate()?;
    let bytes = encode(request)?;
    atomic_write(path, &bytes, None)?;
    Ok(())
}

pub fn encode(request: &SaveProject<'_>) -> Result<Vec<u8>, ProjectError> {
    validate_assets(request.assets, request.embedded)?;
    let manifest = Manifest {
        container_version: CONTAINER_VERSION,
        document_schema_version: SCHEMA_VERSION,
        document_entry: "document.json".into(),
        app_version: request.app_version.into(),
        created_utc: request.created_utc.into(),
        modified_utc: request.modified_utc.into(),
        summary: DocumentSummary {
            title: request.document.title.clone(),
            performers: request.document.performers.len() as u32,
            sets: request.document.sets.len() as u32,
            total_counts: request.document.timeline_counts(),
        },
        assets: request.assets.to_vec(),
    };
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|_| ProjectError::InvalidManifest)?;
    let document_bytes = request
        .document
        .to_json()
        .map_err(|error| ProjectError::Io(error.to_string()))?
        .into_bytes();
    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer.start_file("mimetype", stored)?;
    writer.write_all(MIMETYPE.as_bytes())?;
    writer.start_file("manifest.json", deflated)?;
    writer.write_all(&manifest_bytes)?;
    writer.start_file("document.json", deflated)?;
    writer.write_all(&document_bytes)?;
    for asset in &manifest.assets {
        if let AssetLocation::Embedded { entry } = &asset.location {
            writer.start_file(entry, deflated)?;
            writer.write_all(
                request
                    .embedded
                    .get(&asset.id)
                    .ok_or(ProjectError::MissingEntry("embedded asset"))?,
            )?;
        }
    }
    let bytes = writer.finish()?.into_inner();
    if bytes.len() as u64 > MAX_CONTAINER_BYTES {
        return Err(ProjectError::LimitExceeded("container"));
    }
    Ok(bytes)
}

pub fn load(path: &Path) -> Result<LoadedProject, ProjectError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_CONTAINER_BYTES {
        return Err(ProjectError::LimitExceeded("container"));
    }
    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    if archive.len() > MAX_ENTRIES {
        return Err(ProjectError::LimitExceeded("entries"));
    }
    for index in 0..archive.len() {
        let name = archive.by_index(index)?.name().to_owned();
        validate_entry_name(&name)?;
    }
    let mimetype = read_entry(&mut archive, "mimetype", 256)?;
    if mimetype != MIMETYPE.as_bytes() {
        return Err(ProjectError::InvalidManifest);
    }
    let manifest: Manifest =
        serde_json::from_slice(&read_entry(&mut archive, "manifest.json", 4 * 1024 * 1024)?)
            .map_err(|_| ProjectError::InvalidManifest)?;
    if manifest.container_version > CONTAINER_VERSION {
        return Err(ProjectError::UnsupportedContainer(
            manifest.container_version,
        ));
    }
    validate_entry_name(&manifest.document_entry)?;
    let document = Document::from_json(
        &String::from_utf8(read_entry(
            &mut archive,
            &manifest.document_entry,
            MAX_DOCUMENT_BYTES,
        )?)
        .map_err(|_| ProjectError::InvalidManifest)?,
    )?;
    let mut embedded = BTreeMap::new();
    let mut warnings = Vec::new();
    let project_dir = path.parent().unwrap_or(Path::new("."));
    for asset in &manifest.assets {
        match &asset.location {
            AssetLocation::Embedded { entry } => {
                validate_entry_name(entry)?;
                let bytes = read_entry(&mut archive, entry, MAX_ASSET_BYTES)?;
                verify_asset(asset, &bytes)?;
                embedded.insert(asset.id, bytes);
            }
            AssetLocation::External {
                relative,
                absolute_hint,
            } => {
                let candidate = relative
                    .as_deref()
                    .and_then(|value| safe_relative(value).then(|| project_dir.join(value)))
                    .or_else(|| absolute_hint.as_deref().map(PathBuf::from));
                match candidate.and_then(|path| fs::read(&path).ok().map(|bytes| (path, bytes))) {
                    Some((_path, bytes)) if verify_asset(asset, &bytes).is_ok() => {
                        // Resolved external bytes share the same consumer path
                        // as embedded assets; location remains external in the
                        // manifest and no bytes are copied into the project.
                        embedded.insert(asset.id, bytes);
                    }
                    Some((path, _)) => {
                        warnings.push(LoadWarning::ExternalMismatch { id: asset.id, path })
                    }
                    None => warnings.push(LoadWarning::ExternalMissing {
                        id: asset.id,
                        name: asset.original_name.clone(),
                    }),
                }
            }
        }
    }
    Ok(LoadedProject {
        document,
        manifest,
        embedded,
        warnings,
    })
}

fn validate_assets(
    assets: &[AssetEntry],
    embedded: &BTreeMap<AssetId, Vec<u8>>,
) -> Result<(), ProjectError> {
    if assets.len() > MAX_ENTRIES.saturating_sub(3) {
        return Err(ProjectError::LimitExceeded("assets"));
    }
    let mut previous = None;
    for asset in assets {
        if previous.is_some_and(|id| id >= asset.id) {
            return Err(ProjectError::InvalidManifest);
        }
        previous = Some(asset.id);
        if asset.original_name.len() > 4096 || asset.blake3_hex.len() != 64 {
            return Err(ProjectError::InvalidManifest);
        }
        if let AssetLocation::Embedded { entry } = &asset.location {
            validate_entry_name(entry)?;
            let bytes = embedded
                .get(&asset.id)
                .ok_or(ProjectError::MissingEntry("embedded asset"))?;
            verify_asset(asset, bytes)?;
        }
    }
    Ok(())
}

fn verify_asset(asset: &AssetEntry, bytes: &[u8]) -> Result<(), ProjectError> {
    if bytes.len() as u64 != asset.byte_len
        || blake3::hash(bytes).to_hex().as_str() != asset.blake3_hex
    {
        return Err(ProjectError::HashMismatch(asset.original_name.clone()));
    }
    Ok(())
}

fn validate_entry_name(name: &str) -> Result<(), ProjectError> {
    if !safe_relative(name) || name.contains('\\') {
        Err(ProjectError::UnsafeEntry(name.into()))
    } else {
        Ok(())
    }
}
fn safe_relative(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn read_entry<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, ProjectError> {
    let file = archive
        .by_name(name)
        .map_err(|_| ProjectError::MissingEntry("zip entry"))?;
    if file.size() > limit {
        return Err(ProjectError::LimitExceeded("entry"));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve(file.size() as usize)
        .map_err(|_| ProjectError::LimitExceeded("memory"))?;
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(ProjectError::LimitExceeded("entry"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic_round_trip_with_embedded_asset() {
        let document = Document::demo(2, 2);
        let bytes = b"audio".to_vec();
        let id = AssetId(1);
        let entry = "assets/0001.wav".to_string();
        let asset = AssetEntry {
            id,
            kind: AssetKind::Audio,
            location: AssetLocation::Embedded { entry },
            original_name: "show.wav".into(),
            byte_len: bytes.len() as u64,
            blake3_hex: blake3::hash(&bytes).to_hex().to_string(),
        };
        let embedded = BTreeMap::from([(id, bytes)]);
        let assets = [asset];
        let request = SaveProject {
            document: &document,
            app_version: "test",
            created_utc: "2026-01-01T00:00:00Z",
            modified_utc: "2026-01-01T00:00:00Z",
            embedded: &embedded,
            assets: &assets,
        };
        assert_eq!(encode(&request).unwrap(), encode(&request).unwrap());
        let path = std::env::temp_dir().join(format!(
            "drillforge-container-{}.drillproj",
            std::process::id()
        ));
        save(&path, &request).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.document.performers.len(), 4);
        assert_eq!(loaded.embedded[&id], b"audio");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_external_asset_is_warning_not_failure() {
        let document = Document::demo(1, 1);
        let asset = AssetEntry {
            id: AssetId(2),
            kind: AssetKind::Audio,
            location: AssetLocation::External {
                relative: Some("missing.wav".into()),
                absolute_hint: None,
            },
            original_name: "missing.wav".into(),
            byte_len: 1,
            blake3_hex: blake3::hash(b"x").to_hex().to_string(),
        };
        let request = SaveProject {
            document: &document,
            app_version: "test",
            created_utc: "x",
            modified_utc: "x",
            embedded: &BTreeMap::new(),
            assets: std::slice::from_ref(&asset),
        };
        let path = std::env::temp_dir().join(format!(
            "drillforge-missing-{}.drillproj",
            std::process::id()
        ));
        save(&path, &request).unwrap();
        assert_eq!(load(&path).unwrap().warnings.len(), 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn traversal_is_rejected() {
        assert!(matches!(
            validate_entry_name("../evil"),
            Err(ProjectError::UnsafeEntry(_))
        ));
    }
}
