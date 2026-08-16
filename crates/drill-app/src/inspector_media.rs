//! Inspector panels for media input and deliverable export.
//!
//! Keeping these workflows together makes their state transitions auditable: audio
//! edits always enter `History`, while video export always passes a matching
//! preflight report before a background job can start.

use std::path::PathBuf;

use drill_core::{
    Edit, Locale,
    audio::{AnchorMap, AudioTrack},
    continuity, coordinates, countsheet, svg,
    video::{
        EncoderBackend, ExportPreset, RateControl, VideoCodec, VideoContainer, VideoExportConfig,
    },
};
use eframe::egui::{self, Color32};

use crate::app_state::ViewMode;
use crate::{DrillApp, WorkspaceFocus};

impl DrillApp {
    pub(super) fn show_export_inspector(&mut self, ui: &mut egui::Ui) {
        let focused = self.workspace_focus == Some(WorkspaceFocus::Video);
        egui::CollapsingHeader::new(super::i18n::registered(self.locale, "inspector-media.001"))
            .open(focused.then_some(true))
            .show(ui, |ui| {
                if focused {
                    ui.scroll_to_cursor(Some(egui::Align::Center));
                }
                ui.small(super::i18n::registered(self.locale, "inspector-media.002"));
                ui.collapsing(
                    super::i18n::registered(self.locale, "inspector-media.003"),
                    |ui| self.show_video_export(ui),
                );
                ui.separator();
                self.show_document_exports(ui);
            });
        if focused {
            self.workspace_focus = None;
        }
    }

    fn show_video_export(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(super::i18n::registered(self.locale, "inspector-media.004"))
                .strong(),
        );
        ui.small(super::i18n::registered(self.locale, "inspector-media.005"));
        ui.horizontal_wrapped(|ui| {
            for (preset, label) in [
                (
                    ExportPreset::Fast,
                    super::i18n::registered(self.locale, "inspector-media.006"),
                ),
                (
                    ExportPreset::Standard,
                    super::i18n::registered(self.locale, "inspector-media.007"),
                ),
                (
                    ExportPreset::HighQuality,
                    super::i18n::registered(self.locale, "inspector-media.008"),
                ),
                (ExportPreset::Youtube4k, "YouTube 4K"),
            ] {
                if ui
                    .selectable_value(&mut self.video_preset, preset, label)
                    .clicked()
                {
                    self.video_export = VideoExportConfig::preset(preset);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.video_export.width)
                    .range(320..=7680)
                    .suffix(" W"),
            );
            ui.add(
                egui::DragValue::new(&mut self.video_export.height)
                    .range(240..=4320)
                    .suffix(" H"),
            );
            ui.add(
                egui::DragValue::new(&mut self.video_export.fps)
                    .range(1..=240)
                    .suffix(" fps"),
            );
        });
        ui.horizontal(|ui| {
            ui.label(super::i18n::registered(self.locale, "inspector-media.009"));
            ui.add(egui::Slider::new(&mut self.video_export.quality, 0..=35));
            ui.checkbox(
                &mut self.video_export.audio_enabled,
                super::i18n::registered(self.locale, "inspector-media.010"),
            );
        });
        ui.checkbox(
            &mut self.video_advanced,
            super::i18n::registered(self.locale, "inspector-media.011"),
        );
        if self.video_advanced {
            self.show_advanced_video_settings(ui);
        }

        let duration = self.document.tempo.seconds_at(self.playback_end as f32)
            - self.document.tempo.seconds_at(self.playback_start as f32);
        let speed = if matches!(self.video_export.backend, EncoderBackend::Software) {
            22.0
        } else {
            53.0
        };
        let estimated_seconds =
            (self.video_export.frame_count(duration) as f64 / speed).ceil() as u64;
        ui.small(format!(
            "{}フレーム ・ 推定 {:.1} MB ・ 約{}分{}秒",
            self.video_export.frame_count(duration),
            self.video_export.estimated_megabytes(duration),
            estimated_seconds / 60,
            estimated_seconds % 60,
        ));
        match self.video_export.validate() {
            Ok(()) => {
                ui.colored_label(
                    Color32::from_rgb(99, 210, 151),
                    super::i18n::registered(self.locale, "inspector-media.012"),
                );
            }
            Err(error) => {
                ui.colored_label(Color32::from_rgb(255, 92, 92), error.message(self.locale));
            }
        }
        for issue in drill_export::camera_preflight_issues(
            &self.document,
            drill_export::CountRange {
                start: f64::from(self.playback_start),
                end: f64::from(self.playback_end),
            },
        ) {
            ui.colored_label(
                Color32::from_rgb(255, 184, 77),
                format!("カメラ: {}", issue.message),
            );
        }
        self.show_video_preflight(ui, f64::from(duration));
        self.show_video_run_controls(ui);
    }

    fn show_advanced_video_settings(&mut self, ui: &mut egui::Ui) {
        egui::ComboBox::from_label(super::i18n::registered(self.locale, "inspector-media.013"))
            .selected_text(format!("{:?}", self.video_export.container))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.video_export.container, VideoContainer::Mp4, "MP4");
                ui.selectable_value(&mut self.video_export.container, VideoContainer::Mov, "MOV");
                ui.selectable_value(
                    &mut self.video_export.container,
                    VideoContainer::WebM,
                    "WebM",
                );
            });
        egui::ComboBox::from_label(super::i18n::registered(self.locale, "inspector-media.014"))
            .selected_text(format!("{:?}", self.video_export.codec))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.video_export.codec, VideoCodec::H264, "H.264");
                ui.selectable_value(&mut self.video_export.codec, VideoCodec::H265, "H.265");
                ui.selectable_value(&mut self.video_export.codec, VideoCodec::Av1, "AV1");
                ui.selectable_value(&mut self.video_export.codec, VideoCodec::Vp9, "VP9");
            });
        egui::ComboBox::from_label(super::i18n::registered(self.locale, "inspector-media.015"))
            .selected_text(format!("{:?}", self.video_export.backend))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.video_export.backend,
                    EncoderBackend::Auto,
                    super::i18n::registered(self.locale, "inspector-media.016"),
                );
                ui.selectable_value(
                    &mut self.video_export.backend,
                    EncoderBackend::Software,
                    "CPU",
                );
                ui.selectable_value(
                    &mut self.video_export.backend,
                    EncoderBackend::Nvidia,
                    "NVIDIA",
                );
                ui.selectable_value(
                    &mut self.video_export.backend,
                    EncoderBackend::Intel,
                    "Intel",
                );
                ui.selectable_value(&mut self.video_export.backend, EncoderBackend::Amd, "AMD");
            });
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.video_export.rate_control,
                RateControl::ConstantQuality,
                super::i18n::registered(self.locale, "inspector-media.017"),
            );
            ui.selectable_value(
                &mut self.video_export.rate_control,
                RateControl::Bitrate,
                super::i18n::registered(self.locale, "inspector-media.018"),
            );
        });
        if self.video_export.rate_control == RateControl::Bitrate {
            ui.add(
                egui::DragValue::new(&mut self.video_export.bitrate_kbps)
                    .range(500..=200_000)
                    .suffix(" kbps"),
            );
        }
        ui.add(
            egui::DragValue::new(&mut self.video_export.audio_bitrate_kbps)
                .range(64..=512)
                .suffix(" kbps audio"),
        );
        ui.checkbox(
            &mut self.video_export.faststart,
            super::i18n::registered(self.locale, "inspector-media.019"),
        );
    }

    fn show_video_preflight(&mut self, ui: &mut egui::Ui, duration: f64) {
        if ui
            .add_enabled(
                !self.export_state.is_inspecting(),
                egui::Button::new(if self.export_state.preflight.is_some() {
                    super::i18n::registered(self.locale, "inspector-media.020")
                } else {
                    super::i18n::registered(self.locale, "inspector-media.021")
                }),
            )
            .clicked()
        {
            self.export_state
                .inspect(self.video_export.clone(), duration, None);
        }
        if self.export_state.is_inspecting() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(super::i18n::registered(self.locale, "inspector-media.022"));
            });
        }
        if self.export_state.preflight.is_some()
            && self
                .export_state
                .preflight_for(&self.video_export)
                .is_none()
        {
            ui.colored_label(
                Color32::from_rgb(255, 184, 77),
                super::i18n::registered(self.locale, "inspector-media.023"),
            );
        }
        if let Some(report) = self.export_state.preflight_for(&self.video_export) {
            let color = if report.can_export() {
                Color32::from_rgb(99, 210, 151)
            } else {
                Color32::from_rgb(255, 184, 77)
            };
            ui.colored_label(
                color,
                if report.can_export() {
                    super::i18n::registered(self.locale, "inspector-media.024")
                } else {
                    super::i18n::registered(self.locale, "inspector-media.025")
                },
            );
            ui.small(format!(
                "概算: {:.1} MB ・ 約{}分{}秒",
                report.estimated_bytes as f64 / 1_048_576.0,
                report.estimated_time.as_secs() / 60,
                report.estimated_time.as_secs() % 60,
            ));
            for issue in &report.issues {
                let color = if issue.blocking {
                    Color32::from_rgb(255, 92, 92)
                } else {
                    Color32::from_rgb(255, 184, 77)
                };
                ui.colored_label(color, format!("• {}", issue.message));
            }
            if self.video_advanced {
                for encoder in &report.encoders {
                    ui.small(format!(
                        "{} {:?}: {}",
                        if encoder.available { "✓" } else { "—" },
                        encoder.backend,
                        encoder.reason
                    ));
                }
            }
        }
    }

    fn show_video_run_controls(&mut self, ui: &mut egui::Ui) {
        if self.export_state.is_running() {
            ui.add(
                egui::ProgressBar::new(self.export_state.progress())
                    .show_percentage()
                    .text(super::i18n::registered(self.locale, "inspector-media.026")),
            );
            if ui
                .button(super::i18n::registered(self.locale, "inspector-media.027"))
                .clicked()
            {
                self.export_state.cancel();
            }
        } else if ui
            .button(super::i18n::registered(self.locale, "inspector-media.028"))
            .clicked()
        {
            self.start_video_export_from_dialog();
        }
        if self.export_state.needs_overwrite_confirmation() {
            ui.colored_label(
                Color32::from_rgb(255, 184, 77),
                super::i18n::registered(self.locale, "inspector-media.029"),
            );
            ui.horizontal(|ui| {
                if ui
                    .button(super::i18n::registered(self.locale, "inspector-media.030"))
                    .clicked()
                {
                    self.export_state.confirm_overwrite();
                }
                if ui
                    .button(super::i18n::registered(self.locale, "inspector-media.031"))
                    .clicked()
                {
                    self.export_state.reject_overwrite();
                }
            });
        }
        ui.small(self.export_state.status.text(self.locale));
        if let Some(path) = self.export_state.completed_path() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("完成: {}", path.display()));
                if ui
                    .button(super::i18n::registered(self.locale, "inspector-media.032"))
                    .clicked()
                {
                    let target = path.parent().unwrap_or(path);
                    let _ = std::process::Command::new("explorer").arg(target).spawn();
                }
            });
        }
    }

    fn start_video_export_from_dialog(&mut self) {
        let tools = self
            .export_state
            .preflight_for(&self.video_export)
            .filter(|report| report.can_export())
            .and_then(|report| Some((report.ffmpeg.clone()?, report.ffprobe.clone()?)));
        let Some((ffmpeg, ffprobe)) = tools else {
            self.export_state.status = super::i18n::StatusMessage::new("export-status.016");
            return;
        };
        if let Err(error) = self.video_export.validate() {
            self.export_state.status =
                super::i18n::StatusMessage::new("export-status.017").arg(0, error);
            return;
        }
        if self.playback_end <= self.playback_start {
            self.export_state.status = super::i18n::StatusMessage::new("export-status.018");
            return;
        }
        let Some(output) = rfd::FileDialog::new()
            .add_filter("動画", &[self.video_export.extension()])
            .set_file_name(format!("drill.{}", self.video_export.extension()))
            .save_file()
        else {
            return;
        };
        let audio = self
            .document
            .audio
            .as_ref()
            .filter(|_| self.video_export.audio_enabled)
            .map(|track| PathBuf::from(&track.path));
        self.export_state.start(drill_export::VideoExportRequest {
            document: self.document.clone(),
            config: self.video_export.clone(),
            range: drill_export::CountRange {
                start: f64::from(self.playback_start),
                end: f64::from(self.playback_end),
            },
            output,
            ffmpeg,
            ffprobe,
            audio,
            underlay: self.underlay_state.asset_bytes.clone(),
            use_3d_camera: self.view_mode == ViewMode::Stadium3D,
        });
    }

    fn show_document_exports(&mut self, ui: &mut egui::Ui) {
        if ui
            .button(super::i18n::registered(self.locale, "inspector-media.056"))
            .on_hover_text(super::i18n::registered(self.locale, "inspector-media.057"))
            .clicked()
        {
            self.auto_assign_next();
        }
        if ui
            .button(super::i18n::registered(self.locale, "inspector-media.058"))
            .clicked()
        {
            self.export_text(
                "drill_coordinates.csv",
                "CSV",
                "csv",
                coordinates::coordinates_csv_localized(&self.document, self.locale),
            );
        }
        let first = self.selected.iter().next().copied();
        if ui
            .add_enabled(first.is_some(), egui::Button::new("選択演者のドリルシート"))
            .clicked()
            && let Some(index) = first
        {
            let sheet = coordinates::performer_sheet_localized(&self.document, index, self.locale);
            let name = format!("{}_dotbook.txt", self.document.performers[index].label);
            self.export_text(&name, "テキスト", "txt", sheet);
        }
        ui.separator();
        ui.small("印刷・配布用");
        if ui
            .button(super::i18n::registered(self.locale, "inspector-media.059"))
            .clicked()
        {
            self.print_state.open = true;
        }
        if ui
            .button(super::i18n::registered(self.locale, "inspector-media.065"))
            .on_hover_text(super::i18n::registered(self.locale, "inspector-media.066"))
            .clicked()
        {
            let performer_ids: Vec<_> = self
                .selected
                .iter()
                .filter_map(|&index| self.document.performers.get(index).map(|p| p.id))
                .collect();
            self.mobile_viewer_state.start(
                &self.document,
                performer_ids,
                self.locale,
                self.history.revision(),
            );
        }
        if let Some(progress) = self.mobile_viewer_state.progress() {
            ui.horizontal(|ui| {
                ui.add(egui::ProgressBar::new(progress).show_percentage());
                if ui
                    .button(super::i18n::registered(self.locale, "inspector-media.064"))
                    .clicked()
                {
                    self.mobile_viewer_state.cancel();
                }
            });
        }
        if let Some(event) = self.mobile_viewer_state.poll(self.history.revision()) {
            match event {
                super::mobile_viewer_state::MobileViewerEvent::Ready {
                    html,
                    performer_count,
                } => {
                    self.mobile_viewer_state.last_performer_count = performer_count;
                    self.export_text("practice_viewer.html", "HTML", "html", html);
                }
                super::mobile_viewer_state::MobileViewerEvent::Failed => {
                    self.status =
                        super::i18n::registered(self.locale, "inspector-media.067").into();
                }
                super::mobile_viewer_state::MobileViewerEvent::Cancelled => {}
                super::mobile_viewer_state::MobileViewerEvent::Stale => {}
            }
        }
        if self.mobile_viewer_state.last_performer_count > 0 {
            ui.small(format!(
                "{}: {}",
                super::i18n::registered(self.locale, "inspector-media.068"),
                self.mobile_viewer_state.last_performer_count
            ));
        }
        ui.small("従来形式");
        if ui
            .button(super::i18n::registered(self.locale, "inspector-media.060"))
            .clicked()
        {
            let name = format!("set{}.svg", self.current_set + 1);
            match drill_export::set_svg_with_underlay(
                &self.document,
                self.current_set,
                self.underlay_state.asset_bytes.as_deref(),
            ) {
                Ok(svg) => self.export_text(&name, "SVG", "svg", svg),
                Err(error) => {
                    self.status = format!(
                        "{}: {error}",
                        super::i18n::registered(self.locale, "inspector-media.033")
                    )
                }
            }
        }
        if ui
            .button(super::i18n::registered(self.locale, "inspector-media.061"))
            .clicked()
        {
            self.export_text(
                "coordinate_sheet.html",
                "HTML",
                "html",
                svg::coordinate_sheet_html_localized(&self.document, self.locale),
            );
        }
        if ui
            .button(super::i18n::registered(self.locale, "inspector-media.062"))
            .clicked()
        {
            self.export_text(
                "drill_book.html",
                "HTML",
                "html",
                svg::drill_book_html_localized(&self.document, self.locale),
            );
        }
        if ui
            .button(super::i18n::registered(self.locale, "inspector-media.063"))
            .clicked()
        {
            let text = countsheet::count_sheet_text(&self.document, self.beats_per_measure);
            self.export_text("count_sheet.txt", "テキスト", "txt", text);
        }
        if ui
            .button(super::i18n::registered(self.locale, "inspector-media.034"))
            .clicked()
        {
            let text = drill_core::production::production_sheet_text(&self.document, self.locale);
            self.export_text("production_sheet.tsv", "TSV", "tsv", text);
        }
        if ui
            .add_enabled(
                first.is_some(),
                egui::Button::new("選択演者のコンティニュイティ (TXT)"),
            )
            .clicked()
            && let Some(index) = first
        {
            let text = continuity::continuity_text_localized(&self.document, index, self.locale);
            let name = format!("{}_continuity.txt", self.document.performers[index].label);
            self.export_text(&name, "テキスト", "txt", text);
        }
    }

    pub(super) fn show_audio_inspector(&mut self, ui: &mut egui::Ui) {
        let focused = self.workspace_focus == Some(WorkspaceFocus::Audio);
        egui::CollapsingHeader::new(super::i18n::registered(self.locale, "inspector-media.035"))
            .open(focused.then_some(true))
            .show(ui, |ui| {
                if focused {
                    ui.scroll_to_cursor(Some(egui::Align::Center));
                }
                if !self.audio_draft_dirty && self.audio_draft != self.document.audio {
                    self.audio_draft = self.document.audio.clone();
                }
                ui.horizontal(|ui| {
                    ui.label(super::i18n::registered(self.locale, "inspector-media.036"));
                    ui.add(egui::DragValue::new(&mut self.beats_per_measure).range(1..=16));
                });
                self.show_click_settings(ui);
                self.show_audio_relink(ui);
                if self.document.audio.is_some() {
                    self.show_attached_audio(ui);
                } else {
                    self.show_audio_picker(ui);
                }
                ui.small(self.audio_state.status.text(self.locale));
                if self.audio_state.is_loading()
                    && ui
                        .button(super::i18n::registered(self.locale, "inspector-media.037"))
                        .clicked()
                {
                    self.audio_state.cancel();
                }
                if let Some(track) = &self.document.audio {
                    let global = self
                        .document
                        .global_count(self.current_set, self.count_position);
                    let time =
                        drill_core::audio::count_to_audio_time(track, &self.document.tempo, global);
                    ui.small(format!("音源位置: {time:.2} 秒"));
                }
            });
        if focused {
            self.workspace_focus = None;
        }
    }

    fn show_click_settings(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(
            super::i18n::registered(self.locale, "inspector-media.038"),
            |ui| {
                self.click_settings.beats_per_measure = self.beats_per_measure;
                ui.checkbox(
                    &mut self.click_settings.enabled,
                    super::i18n::registered(self.locale, "inspector-media.039"),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label(super::i18n::registered(self.locale, "inspector-media.040"));
                    for (value, label) in [
                        (drill_audio::CountIn::Two, "2"),
                        (drill_audio::CountIn::Four, "4"),
                        (drill_audio::CountIn::Eight, "8"),
                        (drill_audio::CountIn::Sixteen, "16"),
                    ] {
                        ui.selectable_value(&mut self.click_settings.count_in, value, label);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(super::i18n::registered(self.locale, "inspector-media.041"));
                    for value in [1, 2, 4, 8] {
                        ui.selectable_value(
                            &mut self.click_settings.subdivision,
                            value,
                            format!("×{value}"),
                        );
                    }
                });
                ui.add(
                    egui::Slider::new(&mut self.click_settings.volume, 0.0..=1.0)
                        .text(super::i18n::registered(self.locale, "inspector-media.042")),
                );
                ui.add(
                    egui::Slider::new(&mut self.click_settings.accent_hz, 800.0..=3_000.0)
                        .text(super::i18n::registered(self.locale, "inspector-media.043")),
                );
                ui.add(
                    egui::Slider::new(&mut self.click_settings.duck_gain, 0.0..=1.0)
                        .text(super::i18n::registered(self.locale, "inspector-media.044")),
                );
                let events = self.audio_state.configure_click(
                    &self.document.tempo,
                    f64::from(self.document.timeline_counts()),
                    &self.click_settings,
                );
                ui.small(format!("クリック予定 {events} events"));
                ui.small(super::i18n::registered(self.locale, "inspector-media.045"));
            },
        );
    }

    fn show_audio_relink(&mut self, ui: &mut egui::Ui) {
        if !self.project_warnings.is_empty()
            && ui
                .button(super::i18n::registered(self.locale, "inspector-media.046"))
                .clicked()
            && let Some(path) = Self::pick_audio_file(self.locale)
            && let Some(mut track) = self.document.audio.clone()
        {
            track.path = path.display().to_string();
            if self
                .history
                .execute(
                    &mut self.document,
                    Edit::SetAudioTrack { audio: Some(track) },
                )
                .is_ok()
            {
                self.project_warnings.clear();
                self.audio_state.start_decode(path);
                self.dirty = true;
            }
        }
    }

    fn show_attached_audio(&mut self, ui: &mut egui::Ui) {
        let mut track = self
            .audio_draft
            .clone()
            .or_else(|| self.document.audio.clone())
            .expect("checked by caller");
        ui.small(format!("♪ {}", track.path));
        let mut changed = false;
        let mut commit = false;
        ui.horizontal(|ui| {
            ui.label(super::i18n::registered(self.locale, "inspector-media.047"));
            let response = ui.add(
                egui::DragValue::new(&mut track.offset_seconds)
                    .speed(0.05)
                    .suffix(" s"),
            );
            changed |= response.changed();
            commit |= response.drag_stopped() || (response.changed() && !response.dragged());
        });
        ui.horizontal(|ui| {
            ui.label(super::i18n::registered(self.locale, "inspector-media.048"));
            let response = ui.add(
                egui::DragValue::new(&mut track.duration_seconds)
                    .range(0.0..=100_000.0)
                    .suffix(" s"),
            );
            changed |= response.changed();
            commit |= response.drag_stopped() || (response.changed() && !response.dragged());
        });
        ui.horizontal(|ui| {
            ui.label(super::i18n::registered(self.locale, "inspector-media.049"));
            let gain = ui.add(egui::Slider::new(&mut track.gain_db, -48.0..=12.0).suffix(" dB"));
            changed |= gain.changed();
            commit |= gain.drag_stopped() || (gain.changed() && !gain.dragged());
            let muted = ui.checkbox(
                &mut track.muted,
                super::i18n::registered(self.locale, "inspector-media.050"),
            );
            changed |= muted.changed();
            commit |= muted.changed();
        });
        ui.label(super::i18n::registered(self.locale, "inspector-media.051"));
        ui.horizontal(|ui| {
            let trim_start = ui.add(
                egui::DragValue::new(&mut track.trim_start_seconds)
                    .range(0.0..=track.duration_seconds)
                    .suffix(" in"),
            );
            changed |= trim_start.changed();
            commit |= trim_start.drag_stopped() || (trim_start.changed() && !trim_start.dragged());
            let trim_end = ui.add(
                egui::DragValue::new(&mut track.trim_end_seconds)
                    .range(0.0..=track.duration_seconds)
                    .suffix(" out"),
            );
            changed |= trim_end.changed();
            commit |= trim_end.drag_stopped() || (trim_end.changed() && !trim_end.dragged());
        });
        ui.horizontal(|ui| {
            let fade_in = ui.add(
                egui::DragValue::new(&mut track.fade_in_seconds)
                    .range(0.0..=30.0)
                    .suffix(" fade-in"),
            );
            changed |= fade_in.changed();
            commit |= fade_in.drag_stopped() || (fade_in.changed() && !fade_in.dragged());
            let fade_out = ui.add(
                egui::DragValue::new(&mut track.fade_out_seconds)
                    .range(0.0..=30.0)
                    .suffix(" fade-out"),
            );
            changed |= fade_out.changed();
            commit |= fade_out.drag_stopped() || (fade_out.changed() && !fade_out.dragged());
        });
        match track.validate() {
            Ok(()) => {
                ui.small(format!(
                    "使用範囲 {:.2} 秒・ゲイン ×{:.2}",
                    track.effective_duration(),
                    track.gain_linear()
                ));
            }
            Err(error) => {
                ui.colored_label(Color32::from_rgb(255, 92, 92), error.message(self.locale));
            }
        }
        if changed {
            self.audio_draft = Some(track.clone());
            self.audio_draft_dirty = true;
        }
        if commit
            && self.audio_draft_dirty
            && track.validate().is_ok()
            && self
                .history
                .execute(
                    &mut self.document,
                    Edit::SetAudioTrack {
                        audio: Some(track.clone()),
                    },
                )
                .is_ok()
        {
            self.audio_draft_dirty = false;
            self.audio_draft = Some(track);
            self.dirty = true;
        }
        if ui
            .button(super::i18n::registered(self.locale, "inspector-media.052"))
            .clicked()
            && self
                .history
                .execute(&mut self.document, Edit::SetAudioTrack { audio: None })
                .is_ok()
        {
            self.audio_draft = None;
            self.audio_draft_dirty = false;
            self.dirty = true;
        }
    }

    fn show_audio_picker(&mut self, ui: &mut egui::Ui) {
        if ui
            .button(super::i18n::registered(self.locale, "inspector-media.053"))
            .clicked()
            && let Some(path) = Self::pick_audio_file(self.locale)
        {
            let audio = Some(AudioTrack {
                path: path.display().to_string(),
                duration_seconds: 0.0,
                offset_seconds: 0.0,
                anchors: AnchorMap::default(),
                gain_db: 0.0,
                muted: false,
                trim_start_seconds: 0.0,
                trim_end_seconds: 0.0,
                fade_in_seconds: 0.0,
                fade_out_seconds: 0.0,
            });
            if self
                .history
                .execute(&mut self.document, Edit::SetAudioTrack { audio })
                .is_ok()
            {
                self.dirty = true;
                self.audio_state.start_decode(path);
            }
        }
    }

    fn pick_audio_file(locale: Locale) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter(
                super::i18n::registered(locale, "inspector-media.054"),
                &["wav", "mp3", "ogg", "flac"],
            )
            .pick_file()
    }

    pub(super) fn show_text_export_progress(&mut self, ui: &mut egui::Ui) {
        if let Some(progress) = self.text_export_state.progress() {
            ui.horizontal(|ui| {
                ui.add(
                    egui::ProgressBar::new(progress)
                        .desired_width(140.0)
                        .show_percentage()
                        .text(super::i18n::registered(self.locale, "inspector-media.055")),
                );
                if ui
                    .small_button(super::i18n::registered(self.locale, "inspector-media.069"))
                    .clicked()
                {
                    self.text_export_state.cancel();
                }
            });
        }
    }
}
