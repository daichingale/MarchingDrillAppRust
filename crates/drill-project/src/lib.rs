use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub mod assets;
pub mod container;
pub mod crash;
pub mod journal;
pub mod recovery;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallReport {
    pub destination: PathBuf,
    pub backup: Option<PathBuf>,
    pub replaced_existing: bool,
}

/// Atomically installs an already completed and verified file.
///
/// `staged` must be beside `destination`; this is what makes replacement
/// atomic on OneDrive-backed and ordinary local directories alike. The staged
/// file is synced before replacement. Existing output is never deleted first.
pub fn install_file(
    staged: &Path,
    destination: &Path,
    backup: Option<&Path>,
) -> io::Result<InstallReport> {
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staged_parent = staged
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if staged_parent != parent {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "staged file must be beside the destination",
        ));
    }
    if let Some(backup) = backup {
        let backup_parent = backup
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if backup_parent != parent || backup == destination || backup == staged {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "backup must be a distinct file beside the destination",
            ));
        }
    }
    let metadata = fs::metadata(staged)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "staged path is not a regular file",
        ));
    }
    // On Windows, FlushFileBuffers requires a handle opened with write access;
    // a read-only `File::open` fails with ERROR_ACCESS_DENIED.
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(staged)?
        .sync_all()?;
    let replaced_existing = destination.is_file();
    replace(staged, destination, backup.filter(|_| replaced_existing))?;
    sync_directory(parent)?;
    Ok(InstallReport {
        destination: destination.to_path_buf(),
        backup: backup.filter(|_| replaced_existing).map(Path::to_path_buf),
        replaced_existing,
    })
}

/// Durably writes `contents` before atomically replacing `path`.
///
/// The temporary file is always created beside the destination so the final
/// replacement cannot cross filesystem boundaries. On Windows, `backup` is
/// produced by `ReplaceFileW` as part of the replacement operation.
pub fn atomic_write(path: &Path, contents: &[u8], backup: Option<&Path>) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "destination has no file name")
    })?;
    if let Some(backup) = backup {
        let backup_parent = backup
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if backup_parent != parent {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "backup must be beside the destination",
            ));
        }
    }

    let (temporary_path, mut temporary_file) = create_temporary(parent, file_name)?;
    let mut cleanup = TempCleanup(Some(temporary_path.clone()));
    temporary_file.write_all(contents)?;
    temporary_file.flush()?;
    temporary_file.sync_all()?;
    drop(temporary_file);

    replace(&temporary_path, path, backup)?;
    cleanup.0 = None;
    sync_directory(parent)?;
    if fs::read(path)? != contents {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saved file verification failed",
        ));
    }
    Ok(())
}

fn create_temporary(parent: &Path, file_name: &std::ffi::OsStr) -> io::Result<(PathBuf, File)> {
    for _ in 0..100 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!(
            ".{}.{}.{}.tmp",
            file_name.to_string_lossy(),
            std::process::id(),
            sequence
        );
        let path = parent.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary save file",
    ))
}

struct TempCleanup(Option<PathBuf>);

impl Drop for TempCleanup {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(not(windows))]
fn replace(temporary: &Path, destination: &Path, backup: Option<&Path>) -> io::Result<()> {
    if let Some(backup) = backup
        && destination.exists()
    {
        fs::copy(destination, backup)?;
        File::open(backup)?.sync_all()?;
    }
    fs::rename(temporary, destination)
}

#[cfg(windows)]
fn replace(temporary: &Path, destination: &Path, backup: Option<&Path>) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    unsafe extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut core::ffi::c_void,
            reserved: *mut core::ffi::c_void,
        ) -> i32;
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    const REPLACEFILE_IGNORE_MERGE_ERRORS: u32 = 0x2;
    const REPLACEFILE_IGNORE_ACL_ERRORS: u32 = 0x4;

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let destination_exists = destination.exists();
    let temporary = wide(temporary);
    let destination = wide(destination);
    let backup = backup.map(wide);
    let succeeded = if destination_exists {
        // SAFETY: All pointers reference live, NUL-terminated UTF-16 buffers for
        // the duration of the call. The reserved pointers are required to be null.
        unsafe {
            ReplaceFileW(
                destination.as_ptr(),
                temporary.as_ptr(),
                backup
                    .as_ref()
                    .map_or(std::ptr::null(), |path| path.as_ptr()),
                REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        }
    } else {
        // SAFETY: Both pointers reference live, NUL-terminated UTF-16 buffers.
        unsafe {
            MoveFileExW(
                temporary.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(windows)]
fn sync_directory(_path: &Path) -> io::Result<()> {
    // The temporary file is synced before replacement. Opening a directory for
    // sync on Windows requires a backup-semantics handle.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory(name: &str) -> PathBuf {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "drillforge-atomic-save-{name}-{}-{sequence}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn creates_and_replaces_without_leaving_temporary_files() {
        let directory = test_directory("replace");
        fs::create_dir(&directory).unwrap();
        let destination = directory.join("show.drill.json");

        atomic_write(&destination, b"first", None).unwrap();
        atomic_write(&destination, b"second", None).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"second");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn replacement_can_preserve_the_previous_version() {
        let directory = test_directory("backup");
        fs::create_dir(&directory).unwrap();
        let destination = directory.join("show.drill.json");
        let backup = directory.join("show.backup.drill.json");
        atomic_write(&destination, b"first", None).unwrap();

        atomic_write(&destination, b"second", Some(&backup)).unwrap();

        assert_eq!(fs::read(&destination).unwrap(), b"second");
        assert_eq!(fs::read(&backup).unwrap(), b"first");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failure_does_not_create_a_partial_destination() {
        let directory = test_directory("failure");
        let destination = directory.join("show.drill.json");

        assert!(atomic_write(&destination, b"data", None).is_err());
        assert!(!destination.exists());
    }

    #[test]
    fn rejects_backup_on_another_directory() {
        let directory = test_directory("backup-location");
        let other = test_directory("backup-location-other");
        fs::create_dir(&directory).unwrap();
        fs::create_dir(&other).unwrap();
        let destination = directory.join("show.drill.json");
        let backup = other.join("show.backup.drill.json");

        let error = atomic_write(&destination, b"data", Some(&backup)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(!destination.exists());
        fs::remove_dir_all(directory).unwrap();
        fs::remove_dir_all(other).unwrap();
    }

    #[test]
    fn installs_verified_file_and_preserves_existing_backup() {
        let directory = test_directory("video-install");
        fs::create_dir(&directory).unwrap();
        let destination = directory.join("show.mp4");
        let staged = directory.join(".show.partial.mp4");
        let backup = directory.join("show.backup.mp4");
        fs::write(&destination, b"old-video").unwrap();
        fs::write(&staged, b"verified-video").unwrap();

        let report = install_file(&staged, &destination, Some(&backup)).unwrap();

        assert!(report.replaced_existing);
        assert_eq!(report.backup, Some(backup.clone()));
        assert_eq!(fs::read(destination).unwrap(), b"verified-video");
        assert_eq!(fs::read(backup).unwrap(), b"old-video");
        assert!(!staged.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn onedrive_named_directory_uses_same_directory_atomic_install() {
        let directory = test_directory("OneDrive-video-install");
        fs::create_dir(&directory).unwrap();
        let destination = directory.join("show.mp4");
        let staged = directory.join(".show.partial.mp4");
        fs::write(&staged, b"video").unwrap();

        let report = install_file(&staged, &destination, None).unwrap();

        assert!(!report.replaced_existing);
        assert_eq!(fs::read(destination).unwrap(), b"video");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_install_keeps_staged_data_and_existing_target() {
        let directory = test_directory("install-failure");
        fs::create_dir(&directory).unwrap();
        let destination = directory.join("target-directory");
        fs::create_dir(&destination).unwrap();
        let staged = directory.join(".show.partial.mp4");
        fs::write(&staged, b"video").unwrap();

        assert!(install_file(&staged, &destination, None).is_err());
        assert!(destination.is_dir());
        assert_eq!(fs::read(&staged).unwrap(), b"video");
        fs::remove_dir_all(directory).unwrap();
    }
}
