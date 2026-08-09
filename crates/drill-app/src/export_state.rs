use super::i18n::StatusMessage;
use drill_core::video::VideoExportConfig;
use drill_export::{
    ExportPreflight, ExportReport, ReplacePolicy, VideoExportRequest, inspect_export_environment,
    spawn_video_export, spawn_video_export_with_policy,
};
use drill_jobs::{Job, JobMsg};
use std::path::{Path, PathBuf};

pub(crate) struct ExportState {
    job: Option<Job<ExportReport>>,
    pending_overwrite: Option<VideoExportRequest>,
    preflight_job: Option<Job<ExportPreflight>>,
    pub preflight: Option<ExportPreflight>,
    inspected_config: Option<VideoExportConfig>,
    pub completed: Option<ExportReport>,
    pub status: StatusMessage,
}

impl Default for ExportState {
    fn default() -> Self {
        Self {
            job: None,
            pending_overwrite: None,
            preflight_job: None,
            preflight: None,
            inspected_config: None,
            completed: None,
            status: StatusMessage::new("export-status.001"),
        }
    }
}

impl ExportState {
    pub fn inspect(&mut self, config: VideoExportConfig, duration: f64, output: Option<PathBuf>) {
        self.status = StatusMessage::new("export-status.002");
        self.preflight = None;
        self.inspected_config = Some(config.clone());
        self.preflight_job = Some(Job::spawn_typed(
            drill_jobs::JobKind::ExportVideo,
            move |_| {
                Ok(inspect_export_environment(
                    &config,
                    duration,
                    output.as_deref(),
                ))
            },
        ));
    }

    pub fn is_inspecting(&self) -> bool {
        self.preflight_job.is_some()
    }

    pub fn preflight_for(&self, config: &VideoExportConfig) -> Option<&ExportPreflight> {
        (self.inspected_config.as_ref() == Some(config))
            .then_some(self.preflight.as_ref())
            .flatten()
    }
    pub fn start(&mut self, request: VideoExportRequest) {
        if self.job.is_some() {
            self.status = StatusMessage::new("export-status.003");
            return;
        }
        if request.output.exists() {
            self.pending_overwrite = Some(request);
            self.status = StatusMessage::new("export-status.004");
        } else {
            self.spawn(request);
        }
    }

    fn spawn(&mut self, request: VideoExportRequest) {
        self.status = StatusMessage::new("export-status.005");
        self.completed = None;
        self.job = Some(spawn_video_export(request));
    }

    pub fn confirm_overwrite(&mut self) {
        let Some(request) = self.pending_overwrite.take() else {
            return;
        };
        self.status = StatusMessage::new("export-status.006");
        self.completed = None;
        self.job = Some(spawn_video_export_with_policy(
            request,
            ReplacePolicy::BackupAndReplace,
        ));
    }

    pub fn reject_overwrite(&mut self) {
        self.pending_overwrite = None;
        self.status = StatusMessage::new("export-status.007");
    }
    pub fn needs_overwrite_confirmation(&self) -> bool {
        self.pending_overwrite.is_some()
    }
    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }
    pub fn progress(&self) -> f32 {
        self.job.as_ref().map_or(0.0, Job::progress)
    }
    pub fn cancel(&self) {
        if let Some(job) = &self.job {
            job.cancel();
        }
    }

    pub fn poll(&mut self) {
        if let Some(message) = self.preflight_job.as_mut().and_then(Job::poll) {
            self.preflight_job = None;
            match message {
                JobMsg::Done(report) => {
                    self.status = if report.can_export() {
                        StatusMessage::new("export-status.008")
                    } else {
                        StatusMessage::new("export-status.009")
                    };
                    self.preflight = Some(report);
                }
                JobMsg::Cancelled => self.status = StatusMessage::new("export-status.010"),
                JobMsg::Failed(reason) => {
                    self.status = StatusMessage::new("export-status.011").arg(0, reason)
                }
            }
        }
        let Some(message) = self.job.as_mut().and_then(Job::poll) else {
            return;
        };
        self.job = None;
        self.status = match &message {
            JobMsg::Done(report) if report.used_software_fallback => {
                StatusMessage::new("export-status.012").arg(0, report.output.display())
            }
            JobMsg::Done(report) => StatusMessage::new("export-status.013")
                .arg(0, report.output.display())
                .arg(1, report.frames),
            JobMsg::Cancelled => StatusMessage::new("export-status.014"),
            JobMsg::Failed(reason) => StatusMessage::new("export-status.015").arg(0, reason),
        };
        if let JobMsg::Done(report) = message {
            self.completed = Some(report);
        }
    }

    pub fn completed_path(&self) -> Option<&Path> {
        self.completed
            .as_ref()
            .map(|report| report.output.as_path())
    }
}
