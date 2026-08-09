use drill_core::Locale;
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg};
use drill_updater::Version;
use drill_updater::{Channel, Manifest, Preferences, Release, TransportError, UpdateDecision};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateFailure {
    NotConfigured,
    Offline,
    Timeout,
    Tls,
    Http(u16),
    InvalidResponse,
    Unsupported,
    Other(String),
}

impl From<TransportError> for UpdateFailure {
    fn from(error: TransportError) -> Self {
        match error {
            TransportError::NotConfigured => Self::NotConfigured,
            TransportError::Offline => Self::Offline,
            TransportError::Timeout => Self::Timeout,
            TransportError::Tls => Self::Tls,
            TransportError::HttpStatus(code) => Self::Http(code),
            TransportError::InvalidManifest(_)
            | TransportError::InvalidContentType
            | TransportError::TooLarge
            | TransportError::RedirectRejected => Self::InvalidResponse,
            TransportError::Unsupported => Self::Unsupported,
            TransportError::Platform(code) => Self::Other(format!("OS error {code}")),
        }
    }
}

impl UpdateFailure {
    fn text(&self, locale: Locale) -> String {
        match (self, locale) {
            (Self::NotConfigured, Locale::Ja) => "このビルドには更新先が設定されていません".into(),
            (Self::NotConfigured, Locale::En) => {
                "This build has no configured update endpoint".into()
            }
            (Self::Offline, Locale::Ja) => "ネットワークに接続できません".into(),
            (Self::Offline, Locale::En) => "Network is unavailable".into(),
            (Self::Timeout, Locale::Ja) => "更新サーバーがタイムアウトしました".into(),
            (Self::Timeout, Locale::En) => "The update server timed out".into(),
            (Self::Tls, Locale::Ja) => "安全な接続を検証できませんでした".into(),
            (Self::Tls, Locale::En) => "Secure connection validation failed".into(),
            (Self::Http(code), Locale::Ja) => format!("更新サーバーエラー (HTTP {code})"),
            (Self::Http(code), Locale::En) => format!("Update server error (HTTP {code})"),
            (Self::InvalidResponse, Locale::Ja) => "更新情報が不正です".into(),
            (Self::InvalidResponse, Locale::En) => "The update response is invalid".into(),
            (Self::Unsupported, Locale::Ja) => "この環境では更新確認を利用できません".into(),
            (Self::Unsupported, Locale::En) => {
                "Update checks are unsupported on this platform".into()
            }
            (Self::Other(error), _) => error.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckResult {
    Available(Box<Release>),
    Current,
    Deferred,
    Failed(UpdateFailure),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    Available(Version),
    Current,
    Deferred,
    Failed(UpdateFailure),
    Cancelled,
    Skipped,
    DownloadAddressCopied,
}

impl UpdateStatus {
    pub fn text(&self, locale: Locale) -> String {
        use UpdateStatus::*;
        match (self, locale) {
            (Idle, _) => String::new(),
            (Checking, Locale::Ja) => "更新を確認しています…".into(),
            (Checking, Locale::En) => "Checking for updates…".into(),
            (Available(version), Locale::Ja) => format!("DrillForge {version} を利用できます"),
            (Available(version), Locale::En) => format!("DrillForge {version} is available"),
            (Current, Locale::Ja) => "最新版です".into(),
            (Current, Locale::En) => "DrillForge is up to date".into(),
            (Deferred, Locale::Ja) => "更新は延期されています".into(),
            (Deferred, Locale::En) => "The update is deferred".into(),
            (Failed(error), Locale::Ja) => {
                format!("更新を確認できませんでした: {}", error.text(locale))
            }
            (Failed(error), Locale::En) => {
                format!("Could not check for updates: {}", error.text(locale))
            }
            (Cancelled, Locale::Ja) => "更新確認をキャンセルしました".into(),
            (Cancelled, Locale::En) => "Update check cancelled".into(),
            (Skipped, Locale::Ja) => "このバージョンをスキップしました".into(),
            (Skipped, Locale::En) => "This version will be skipped".into(),
            (DownloadAddressCopied, Locale::Ja) => {
                "ダウンロード先をコピーしました。ブラウザで確認してください".into()
            }
            (DownloadAddressCopied, Locale::En) => {
                "Download address copied. Review it in your browser.".into()
            }
        }
    }
}

#[derive(Default)]
pub struct UpdateState {
    job: Option<Job<CheckResult>>,
    pub preferences: Preferences,
    pub available: Option<Release>,
    pub status: UpdateStatus,
}

impl UpdateState {
    /// Starts a background check. `None` uses the fixed, HTTPS-only production
    /// transport. Supplied bytes exist for deterministic conformance/testing.
    pub fn check(&mut self, manifest_bytes: Option<Vec<u8>>, month: u8, day: u8) {
        let preferences = self.preferences.clone();
        self.status = UpdateStatus::Checking;
        self.job = Some(Job::spawn_typed(JobKind::UpdateCheck, move |_| {
            let manifest = if let Some(bytes) = manifest_bytes {
                match Manifest::parse(&bytes) {
                    Ok(manifest) => manifest,
                    Err(_) => return Ok(CheckResult::Failed(UpdateFailure::InvalidResponse)),
                }
            } else {
                match drill_updater::fetch_manifest() {
                    Ok(manifest) => manifest,
                    Err(error) => return Ok(CheckResult::Failed(error.into())),
                }
            };
            let current = Version::parse(env!("CARGO_PKG_VERSION"))
                .map_err(|_| JobFailure::new(JobErrorCode::InvalidInput))?;
            Ok(
                match drill_updater::decide(&current, &manifest, &preferences, month, day) {
                    UpdateDecision::Available(release) => CheckResult::Available(release),
                    UpdateDecision::Current | UpdateDecision::DowngradeRejected => {
                        CheckResult::Current
                    }
                    UpdateDecision::Skipped | UpdateDecision::BlackoutDeferred => {
                        CheckResult::Deferred
                    }
                },
            )
        }));
    }

    pub fn poll(&mut self) {
        let Some(message) = self.job.as_mut().and_then(Job::poll) else {
            return;
        };
        self.job = None;
        match message {
            JobMsg::Done(CheckResult::Available(release)) => {
                self.status = UpdateStatus::Available(release.version.clone());
                self.available = Some(*release);
            }
            JobMsg::Done(CheckResult::Current) => self.status = UpdateStatus::Current,
            JobMsg::Done(CheckResult::Deferred) => self.status = UpdateStatus::Deferred,
            JobMsg::Done(CheckResult::Failed(error)) => self.status = UpdateStatus::Failed(error),
            JobMsg::Failed(error) => {
                self.status = UpdateStatus::Failed(UpdateFailure::Other(error.to_string()))
            }
            JobMsg::Cancelled => self.status = UpdateStatus::Cancelled,
        }
    }

    pub fn skip_available(&mut self) {
        if let Some(release) = self.available.take() {
            self.preferences.skipped_version = Some(release.version);
            self.status = UpdateStatus::Skipped;
        }
    }

    pub fn set_beta(&mut self, enabled: bool) {
        self.preferences.channel = if enabled {
            Channel::Beta
        } else {
            Channel::Stable
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drill_updater::{Artifact, Manifest};
    use std::time::{Duration, Instant};

    fn wait(state: &mut UpdateState) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while state.job.is_some() && Instant::now() < deadline {
            state.poll();
            std::thread::yield_now();
        }
    }

    #[test]
    fn invalid_supplied_response_is_non_fatal_and_structured() {
        let mut state = UpdateState::default();
        state.check(Some(b"not-json".to_vec()), 1, 1);
        wait(&mut state);
        assert_eq!(
            state.status,
            UpdateStatus::Failed(UpdateFailure::InvalidResponse)
        );
        assert!(!state.status.text(Locale::En).contains('更'));
        assert!(state.available.is_none());
    }

    #[test]
    fn supplied_manifest_is_checked_off_ui_thread() {
        let digest = drill_updater::sha256_hex(b"x");
        let release = Release {
            version: Version::new(9, 0, 0),
            artifact: Artifact {
                download_url: "https://example.test/app".into(),
                sha256: digest.clone(),
                attestation_url: "https://example.test/proof".into(),
                attestation_sha256: digest,
                signer_identity: "release".into(),
            },
            release_notes_ja: "更新".into(),
            release_notes_en: "Update".into(),
            emergency_hotfix: false,
        };
        let bytes = serde_json::to_vec(&Manifest {
            schema_version: 1,
            stable: release,
            beta: None,
        })
        .unwrap();
        let mut state = UpdateState::default();
        state.check(Some(bytes), 1, 1);
        wait(&mut state);
        assert_eq!(
            state.available.as_ref().unwrap().version,
            Version::new(9, 0, 0)
        );
    }

    #[test]
    fn every_update_status_has_an_english_safe_rendering() {
        let statuses = [
            UpdateStatus::Checking,
            UpdateStatus::Available(Version::new(2, 3, 4)),
            UpdateStatus::Current,
            UpdateStatus::Deferred,
            UpdateStatus::Failed(UpdateFailure::Offline),
            UpdateStatus::Cancelled,
            UpdateStatus::Skipped,
            UpdateStatus::DownloadAddressCopied,
        ];
        for status in statuses {
            let english = status.text(Locale::En);
            assert!(!english.is_empty());
            assert!(
                english
                    .chars()
                    .all(|ch| ch.is_ascii() || matches!(ch, '—' | '…')),
                "English update status leaked non-English text: {english}"
            );
            assert!(!status.text(Locale::Ja).is_empty());
        }
    }
}
