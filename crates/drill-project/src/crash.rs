//! Privacy-conscious crash reports that never touch a user's project file.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_REPORT_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReport {
    pub app_version: String,
    pub unix_ms: u64,
    pub operating_system: String,
    pub architecture: String,
    pub thread: String,
    pub location: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrashReportFile {
    pub path: PathBuf,
    pub report: CrashReport,
}

/// Installs a hook which writes only diagnostic metadata under app data.
/// It deliberately has no document/path argument, so a panic can never overwrite
/// or disclose the open project. The platform's previous hook still runs.
pub fn install_hook(app_data: PathBuf, app_version: &'static str) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = write_report(&app_data, app_version, info);
        previous(info);
    }));
}

fn write_report(
    app_data: &Path,
    app_version: &str,
    info: &PanicHookInfo<'_>,
) -> io::Result<PathBuf> {
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-text panic")
        .chars()
        .take(2048)
        .collect();
    let report = CrashReport {
        app_version: app_version.chars().take(64).collect(),
        unix_ms,
        operating_system: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        thread: std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .chars()
            .take(128)
            .collect(),
        location: info
            .location()
            .map(|value| format!("{}:{}:{}", value.file(), value.line(), value.column())),
        message,
    };
    write_report_value(app_data, &report)
}

fn write_report_value(app_data: &Path, report: &CrashReport) -> io::Result<PathBuf> {
    let directory = app_data.join("crashes");
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!(
        "crash-{}-{}.json",
        report.unix_ms,
        std::process::id()
    ));
    let bytes = serde_json::to_vec_pretty(report).map_err(io::Error::other)?;
    super::atomic_write(&path, &bytes, None)?;
    Ok(path)
}

pub fn scan_reports(app_data: &Path, limit: usize) -> Vec<CrashReportFile> {
    let Ok(entries) = fs::read_dir(app_data.join("crashes")) else {
        return Vec::new();
    };
    let mut reports = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() || metadata.len() > MAX_REPORT_BYTES {
                return None;
            }
            let bytes = fs::read(&path).ok()?;
            let report = serde_json::from_slice(&bytes).ok()?;
            Some(CrashReportFile { path, report })
        })
        .collect::<Vec<_>>();
    reports.sort_by_key(|entry| std::cmp::Reverse(entry.report.unix_ms));
    reports.truncate(limit);
    reports
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("drillforge-crash-{name}-{}", std::process::id()))
    }

    #[test]
    fn report_is_isolated_from_project_and_discoverable() {
        let root = temp("isolated");
        let project = root.join("show.drillproj");
        fs::create_dir_all(&root).unwrap();
        fs::write(&project, b"original").unwrap();
        let report = CrashReport {
            app_version: "test".into(),
            unix_ms: 42,
            operating_system: "test".into(),
            architecture: "test".into(),
            thread: "main".into(),
            location: None,
            message: "boom".into(),
        };
        let report_path = write_report_value(&root, &report).unwrap();
        assert_eq!(fs::read(project).unwrap(), b"original");
        assert!(report_path.starts_with(root.join("crashes")));
        assert_eq!(scan_reports(&root, 10)[0].report, report);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_and_oversized_reports_are_ignored() {
        let root = temp("invalid");
        let directory = root.join("crashes");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("bad.json"), b"not json").unwrap();
        fs::write(
            directory.join("huge.json"),
            vec![b'x'; MAX_REPORT_BYTES as usize + 1],
        )
        .unwrap();
        assert!(scan_reports(&root, 10).is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
