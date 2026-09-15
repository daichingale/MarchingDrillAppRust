use super::i18n::StatusMessage;
use drill_audio::{
    AudioAsset, AudioOutput, DecodeLimits, DecodeOutput, Peak, PeakPyramid, ProgressSink,
    decode_bytes, decode_file,
};
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind, JobMsg, ProgressHandle};
use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) enum WaveformAction {
    Add(drill_core::audio::SyncAnchor),
    Move {
        id: drill_core::audio::AnchorId,
        anchor: drill_core::audio::SyncAnchor,
    },
    Remove(drill_core::audio::AnchorId),
}

fn waveform_bucket_count(width: f32) -> usize {
    width.max(1.0).round().clamp(1.0, 2048.0) as usize
}

fn count_from_x(x: f32, left: f32, width: f32, start: f32, end: f32) -> f32 {
    start + ((x - left) / width.max(1.0)).clamp(0.0, 1.0) * (end - start).max(0.0)
}

struct DecodeProgress(ProgressHandle);

impl ProgressSink for DecodeProgress {
    fn set_progress(&self, value: u32) {
        self.0.set(value as f32 / 10_000.0);
    }
    fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

pub(crate) enum DecodeEvent {
    Ready { duration: f32 },
    Failed(String),
    Cancelled,
}

pub(crate) struct AudioState {
    job: Option<Job<DecodeOutput>>,
    asset: Option<Arc<AudioAsset>>,
    peaks: Option<PeakPyramid>,
    output: Option<AudioOutput>,
    buckets: Vec<Peak>,
    click_schedule: drill_audio::ClickSchedule,
    last_click_config: Option<(
        drill_audio::ClickSettings,
        f64,
        drill_core::tempo::TempoMap,
        u32,
    )>,
    pub status: StatusMessage,
}

impl Default for AudioState {
    fn default() -> Self {
        Self {
            job: None,
            asset: None,
            peaks: None,
            output: None,
            buckets: Vec::new(),
            click_schedule: drill_audio::ClickSchedule::default(),
            last_click_config: None,
            status: StatusMessage::new("audio-status.001"),
        }
    }
}

impl AudioState {
    pub fn start_decode(&mut self, path: PathBuf) {
        if let Some(job) = &self.job {
            job.cancel();
        }
        self.output = None;
        self.asset = None;
        self.peaks = None;
        self.last_click_config = None;
        self.status = StatusMessage::new("audio-status.002");
        self.job = Some(Job::spawn_typed(JobKind::AudioDecode, move |progress| {
            decode_file(
                &path,
                DecodeLimits::default(),
                &DecodeProgress(progress.clone()),
            )
            .map_err(|_| JobFailure::new(JobErrorCode::Decode))
        }));
    }

    pub fn start_decode_bytes(&mut self, bytes: Vec<u8>, original_name: &str) {
        if let Some(job) = &self.job {
            job.cancel();
        }
        self.output = None;
        self.asset = None;
        self.peaks = None;
        self.last_click_config = None;
        self.status = StatusMessage::new("audio-status.003");
        let extension = PathBuf::from(original_name)
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_owned);
        self.job = Some(Job::spawn_typed(JobKind::AudioDecode, move |progress| {
            decode_bytes(
                bytes,
                extension.as_deref(),
                DecodeLimits::default(),
                &DecodeProgress(progress.clone()),
            )
            .map_err(|_| JobFailure::new(JobErrorCode::Decode))
        }));
    }

    pub fn poll(&mut self) -> Option<DecodeEvent> {
        let job = self.job.as_mut()?;
        self.status =
            StatusMessage::new("audio-status.004").arg(0, format!("{:.0}", job.progress() * 100.0));
        match job.poll()? {
            JobMsg::Done(decoded) => {
                let duration = decoded.asset.duration_seconds() as f32;
                let asset = Arc::new(decoded.asset);
                self.output = match AudioOutput::open_default(Arc::clone(&asset)) {
                    Ok(output) => {
                        self.status = StatusMessage::new("audio-status.005");
                        Some(output)
                    }
                    Err(error) => {
                        self.status = StatusMessage::new("audio-status.006").arg(0, error);
                        None
                    }
                };
                self.asset = Some(asset);
                self.peaks = Some(decoded.peaks);
                // Force one output-program update on the next UI pass. A matching
                // schedule may already have been built while decode was running.
                self.last_click_config = None;
                self.job = None;
                Some(DecodeEvent::Ready { duration })
            }
            JobMsg::Failed(error) => {
                self.job = None;
                self.status = StatusMessage::new("audio-status.007").arg(0, &error);
                Some(DecodeEvent::Failed(error.to_string()))
            }
            JobMsg::Cancelled => {
                self.job = None;
                self.status = StatusMessage::new("audio-status.008");
                Some(DecodeEvent::Cancelled)
            }
        }
    }

    pub fn cancel(&self) {
        if let Some(job) = &self.job {
            job.cancel();
        }
    }
    pub fn is_loading(&self) -> bool {
        self.job.is_some()
    }
    pub fn play(&self) {
        if let Some(output) = &self.output {
            output.play();
        }
    }
    pub fn pause(&self) {
        if let Some(output) = &self.output {
            output.pause();
        }
    }
    pub fn seek_seconds(&self, seconds: f32) {
        if let (Some(output), Some(asset)) = (&self.output, &self.asset) {
            output.seek((seconds.max(0.0) * asset.sample_rate() as f32).round() as u64);
        }
    }
    pub fn set_mix(&self, gain: f32, muted: bool) {
        if let Some(output) = &self.output {
            output.set_gain(gain);
            output.set_muted(muted);
        }
    }
    pub fn configure_click(
        &mut self,
        tempo: &drill_core::tempo::TempoMap,
        end_count: f64,
        settings: &drill_audio::ClickSettings,
    ) -> usize {
        let rate = self.output.as_ref().map_or_else(
            || {
                self.asset
                    .as_ref()
                    .map_or(48_000, |asset| asset.sample_rate())
            },
            AudioOutput::output_sample_rate,
        );
        // The full tempo map (not just its bpm at count 0) must be part of the
        // signature: a tempo change anchored anywhere after count 0 needs to
        // invalidate the cached click schedule too, or the click track can go
        // stale after a mid-show tempo edit.
        let signature = (*settings, end_count, tempo.clone(), rate);
        if self.last_click_config.as_ref() == Some(&signature) {
            return self.click_schedule.events().len();
        }
        drill_audio::ClickSchedule::build_into(
            tempo,
            0.0,
            end_count,
            settings,
            rate,
            &mut self.click_schedule,
        );
        if let Some(output) = &self.output {
            if settings.enabled {
                let voices = drill_audio::ClickVoices::render(settings, rate);
                if let Err(error) =
                    output.set_clicks(*settings, self.click_schedule.clone(), voices)
                {
                    self.status = StatusMessage::new("audio-status.009").arg(0, error);
                }
            } else {
                output.clear_clicks();
            }
        }
        self.last_click_config = Some(signature);
        self.click_schedule.events().len()
    }
    pub fn clock_seconds(&self) -> Option<f32> {
        let sample = self.output.as_ref()?.clock().sample();
        (sample.sample_rate > 0 && sample.playing)
            .then_some(sample.position as f32 / sample.sample_rate as f32)
    }

    pub fn show_waveform(
        &mut self,
        ui: &mut egui::Ui,
        locale: drill_core::Locale,
        track: Option<&drill_core::audio::AudioTrack>,
        tempo: &drill_core::tempo::TempoMap,
        playhead_count: f32,
        visible_counts: std::ops::RangeInclusive<f32>,
    ) -> Option<WaveformAction> {
        let (Some(asset), Some(peaks), Some(track)) = (&self.asset, &self.peaks, track) else {
            return None;
        };
        let width = ui.available_width().max(1.0);
        let buckets = waveform_bucket_count(width);
        let visible_start = *visible_counts.start();
        let visible_end = *visible_counts.end();
        let visible_span = (visible_end - visible_start).max(f32::EPSILON);
        let start_seconds = drill_core::audio::count_to_audio_time(track, tempo, visible_start)
            .clamp(0.0, track.duration_seconds);
        let end_seconds = drill_core::audio::count_to_audio_time(track, tempo, visible_end)
            .clamp(start_seconds, track.duration_seconds);
        let start_frame = ((start_seconds * asset.sample_rate() as f32).round() as u64)
            .min(asset.frames().saturating_sub(1));
        let end_frame = ((end_seconds * asset.sample_rate() as f32).round() as u64)
            .max(start_frame.saturating_add(1))
            .min(asset.frames());
        peaks.range(asset, start_frame, end_frame, buckets, &mut self.buckets);
        let (response, painter) = ui.allocate_painter(Vec2::new(width, 54.0), Sense::hover());
        painter.rect_filled(response.rect, 3.0, Color32::from_rgb(15, 22, 30));
        let channels = usize::from(asset.channels());
        for (index, pair) in self.buckets.chunks(channels).enumerate() {
            let peak = pair.iter().fold(Peak::SILENT, |a, b| a.merge(*b));
            let x = response.rect.left() + index as f32 / buckets as f32 * response.rect.width();
            let mid = response.rect.center().y;
            let top = mid - peak.max as f32 / 32768.0 * response.rect.height() * 0.45;
            let bottom = mid - peak.min as f32 / 32768.0 * response.rect.height() * 0.45;
            painter.line_segment(
                [Pos2::new(x, top), Pos2::new(x, bottom)],
                Stroke::new(1.0, Color32::from_rgb(91, 190, 235)),
            );
        }
        let x = response.rect.left()
            + ((playhead_count - visible_start) / visible_span) * response.rect.width();
        painter.line_segment(
            [
                Pos2::new(x, response.rect.top()),
                Pos2::new(x, response.rect.bottom()),
            ],
            Stroke::new(2.0, Color32::from_rgb(255, 92, 92)),
        );
        let mut action = None;
        let mut anchor_hovered = false;
        for (index, anchor) in track.anchors.anchors().iter().enumerate() {
            let Some(id) = track.anchors.id_at(index) else {
                continue;
            };
            let x = response.rect.left()
                + ((anchor.count as f32 - visible_start) / visible_span) * response.rect.width();
            if x < response.rect.left() - 8.0 || x > response.rect.right() + 8.0 {
                continue;
            }
            let hit = egui::Rect::from_center_size(
                Pos2::new(x, response.rect.center().y),
                Vec2::new(16.0, response.rect.height()),
            );
            let handle = ui
                .interact(hit, response.id.with(id.get()), Sense::click_and_drag())
                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                .on_hover_text(format!(
                    "Count {:.2} · {:.3}s",
                    anchor.count, anchor.seconds
                ));
            anchor_hovered |= handle.hovered() || handle.dragged();
            painter.line_segment(
                [
                    Pos2::new(x, response.rect.top()),
                    Pos2::new(x, response.rect.bottom()),
                ],
                Stroke::new(2.0, Color32::from_rgb(245, 197, 66)),
            );
            painter.circle_filled(
                Pos2::new(x, response.rect.top() + 7.0),
                5.0,
                Color32::from_rgb(245, 197, 66),
            );
            if handle.drag_stopped()
                && let Some(pointer) = handle.interact_pointer_pos()
            {
                let count = count_from_x(
                    pointer.x,
                    response.rect.left(),
                    response.rect.width(),
                    visible_start,
                    visible_end,
                );
                let seconds = drill_core::audio::count_to_audio_time(track, tempo, count);
                action = Some(WaveformAction::Move {
                    id,
                    anchor: drill_core::audio::SyncAnchor {
                        count: if ui.input(|input| input.modifiers.alt) {
                            f64::from(count)
                        } else {
                            f64::from(count.round())
                        },
                        seconds: f64::from(seconds),
                    },
                });
            }
            handle.context_menu(|ui| {
                let delete_label = match locale {
                    drill_core::Locale::Ja => "同期アンカーを削除",
                    drill_core::Locale::En => "Delete Sync Anchor",
                };
                if ui.button(delete_label).clicked() {
                    action = Some(WaveformAction::Remove(id));
                    ui.close();
                }
            });
        }
        if response.clicked()
            && !anchor_hovered
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let count = count_from_x(
                pointer.x,
                response.rect.left(),
                response.rect.width(),
                visible_start,
                visible_end,
            );
            let seconds = drill_core::audio::count_to_audio_time(track, tempo, count);
            action = Some(WaveformAction::Add(drill_core::audio::SyncAnchor {
                count: if ui.input(|input| input.modifiers.alt) {
                    f64::from(count)
                } else {
                    f64::from(count.round())
                },
                seconds: f64::from(seconds),
            }));
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::{count_from_x, waveform_bucket_count};

    #[test]
    fn waveform_work_is_bounded_by_visible_width() {
        assert_eq!(waveform_bucket_count(640.4), 640);
        assert_eq!(waveform_bucket_count(100_000.0), 2048);
        assert_eq!(waveform_bucket_count(0.0), 1);
    }

    #[test]
    fn waveform_x_maps_to_visible_count_viewport() {
        assert_eq!(count_from_x(50.0, 0.0, 100.0, 40.0, 60.0), 50.0);
        assert_eq!(count_from_x(-1.0, 0.0, 100.0, 40.0, 60.0), 40.0);
        assert_eq!(count_from_x(101.0, 0.0, 100.0, 40.0, 60.0), 60.0);
    }
}
