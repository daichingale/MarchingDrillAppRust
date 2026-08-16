//! Append-only edit journal with per-record integrity checks.

use drill_core::{Document, Edit};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

pub const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_REPLAY_RECORDS: u64 = 100_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JournalRecord {
    pub seq: u64,
    pub base_revision: u64,
    pub edit: Edit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Envelope {
    hash: String,
    record: JournalRecord,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReplayReport {
    pub applied: u64,
    pub skipped: u64,
    pub stopped_at: Option<u64>,
}

pub struct Journal {
    path: PathBuf,
    writer: BufWriter<File>,
    next_seq: u64,
}

impl Journal {
    pub fn open(path: &Path, next_seq: u64) -> io::Result<Self> {
        let parent = path.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            writer: BufWriter::with_capacity(64 * 1024, file),
            next_seq,
        })
    }

    pub fn append(&mut self, base_revision: u64, edit: Edit) -> io::Result<u64> {
        let seq = self.next_seq;
        self.next_seq = self
            .next_seq
            .checked_add(1)
            .ok_or_else(|| io::Error::other("journal sequence exhausted"))?;
        let record = JournalRecord {
            seq,
            base_revision,
            edit,
        };
        let canonical = serde_json::to_vec(&record)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if canonical.len() > MAX_RECORD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "journal record too large",
            ));
        }
        let envelope = Envelope {
            hash: blake3::hash(&canonical).to_hex().to_string(),
            record,
        };
        serde_json::to_writer(&mut self.writer, &envelope).map_err(io::Error::other)?;
        self.writer.write_all(b"\n")?;
        Ok(seq)
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_data()
    }
    pub fn truncate(mut self) -> io::Result<()> {
        self.writer.flush()?;
        drop(self.writer);
        File::create(&self.path)?.sync_all()
    }
}

pub fn replay(path: &Path, document: &mut Document, expected_base_revision: u64) -> ReplayReport {
    let Ok(file) = File::open(path) else {
        return ReplayReport::default();
    };
    let mut report = ReplayReport::default();
    let mut expected_seq = 0u64;
    let mut revision = expected_base_revision;
    for line in BufReader::new(file)
        .lines()
        .take(MAX_REPLAY_RECORDS as usize)
    {
        let Ok(line) = line else {
            report.stopped_at = Some(expected_seq);
            break;
        };
        if line.len() > MAX_RECORD_BYTES.saturating_mul(2) {
            report.stopped_at = Some(expected_seq);
            break;
        }
        let Ok(envelope) = serde_json::from_str::<Envelope>(&line) else {
            report.stopped_at = Some(expected_seq);
            break;
        };
        let Ok(canonical) = serde_json::to_vec(&envelope.record) else {
            report.stopped_at = Some(expected_seq);
            break;
        };
        if envelope.record.seq != expected_seq
            || envelope.record.base_revision != revision
            || envelope.hash != blake3::hash(&canonical).to_hex().as_str()
        {
            report.skipped = report.skipped.saturating_add(1);
            report.stopped_at = Some(envelope.record.seq);
            break;
        }
        if envelope.record.edit.apply(document).is_err() {
            report.skipped = report.skipped.saturating_add(1);
            report.stopped_at = Some(envelope.record.seq);
            break;
        }
        report.applied = report.applied.saturating_add(1);
        expected_seq = expected_seq.saturating_add(1);
        revision = revision.saturating_add(1);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use drill_core::{GridConfig, SCHEMA_VERSION};
    use std::sync::atomic::{AtomicU64, Ordering};
    static ID: AtomicU64 = AtomicU64::new(0);
    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "drillforge-journal-{name}-{}-{}.jsonl",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn edits_replay_in_order_and_truncate() {
        let path = path("replay");
        let mut journal = Journal::open(&path, 0).unwrap();
        journal
            .append(
                7,
                Edit::RenameDocument {
                    title: "Recovered".into(),
                },
            )
            .unwrap();
        journal
            .append(
                8,
                Edit::ReplaceGrid {
                    grid: GridConfig::indoor(),
                    scale_positions: false,
                },
            )
            .unwrap();
        journal.flush().unwrap();
        let mut document = Document::demo(1, 1);
        assert_eq!(document.schema_version, SCHEMA_VERSION);
        let report = replay(&path, &mut document, 7);
        assert_eq!(report.applied, 2);
        assert_eq!(document.title, "Recovered");
        assert_eq!(document.grid.width, 90.0);
        journal.truncate().unwrap();
        assert_eq!(fs::metadata(&path).unwrap().len(), 0);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn corrupt_tail_stops_without_undoing_valid_prefix() {
        let path = path("corrupt");
        let mut journal = Journal::open(&path, 0).unwrap();
        journal
            .append(
                0,
                Edit::RenameDocument {
                    title: "Safe".into(),
                },
            )
            .unwrap();
        journal.flush().unwrap();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{broken\n")
            .unwrap();
        let mut document = Document::demo(1, 1);
        let report = replay(&path, &mut document, 0);
        assert_eq!(report.applied, 1);
        assert_eq!(report.stopped_at, Some(1));
        assert_eq!(document.title, "Safe");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn base_revision_mismatch_applies_nothing() {
        let path = path("revision");
        let mut journal = Journal::open(&path, 0).unwrap();
        journal
            .append(
                5,
                Edit::RenameDocument {
                    title: "Wrong".into(),
                },
            )
            .unwrap();
        journal.flush().unwrap();
        let mut document = Document::demo(1, 1);
        let before = document.title.clone();
        assert_eq!(replay(&path, &mut document, 4).applied, 0);
        assert_eq!(document.title, before);
        fs::remove_file(path).unwrap();
    }
}
