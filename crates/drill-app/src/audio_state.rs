use super::i18n::StatusMessage;
use drill_audio::{
    AudioAsset, AudioOutput, DecodeLimits, DecodeOutput, Peak, PeakPyramid, ProgressSink,
    decode_bytes, decode_file, suggest_bpm,
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

struct WaveCache {
    key: (u32, u32, u32, u32, u64),
    columns: Vec<(f32, f32)>,
}

struct CarrierJob {
    generation: u32,
    job: Job<AudioOutput>,
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
        u64,
    )>,
    bpm_guess: Option<f32>,
    wave_cache: Option<WaveCache>,
    carrier: Option<CarrierJob>,
    media_generation: u32,
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
            bpm_guess: None,
            wave_cache: None,
            carrier: None,
            media_generation: 0,
            status: StatusMessage::new("audio-status.001"),
        }
    }
}

impl AudioState {
    pub fn start_decode(&mut self, path: PathBuf) {
        if let Some(job) = &self.job {
            job.cancel();
        }
        self.forget_media();
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
        self.forget_media();
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
                self.remember_asset(asset, decoded.peaks);
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

    /// Drop the decoded song and the device. Safe when no output was opened.
    pub fn clear(&mut self) {
        self.pause();
        self.cancel();
        self.job = None;
        self.forget_media();
        self.buckets.clear();
        self.status = StatusMessage::new("audio-status.001");
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
    fn forget_media(&mut self) {
        self.media_generation = self.media_generation.wrapping_add(1);
        if let Some(carrier) = &self.carrier {
            carrier.job.cancel();
        }
        self.carrier = None;
        self.output = None;
        self.asset = None;
        self.peaks = None;
        self.bpm_guess = None;
        self.wave_cache = None;
        self.last_click_config = None;
    }

    fn remember_asset(&mut self, asset: Arc<AudioAsset>, peaks: PeakPyramid) {
        self.bpm_guess = suggest_bpm(&asset).map(|guess| guess.bpm);
        self.wave_cache = None;
        self.asset = Some(asset);
        self.peaks = Some(peaks);
    }

    /// Suggested constant tempo from the loaded song, in BPM.
    #[must_use]
    pub fn bpm_guess(&self) -> Option<f32> {
        self.bpm_guess
    }

    /// Install a decoded asset without opening a device. Used by the
    /// screenshot harness so the timeline can draw a waveform headlessly.
    pub fn install_preview(&mut self, asset: AudioAsset) {
        let peaks = PeakPyramid::build(&asset);
        self.remember_asset(Arc::new(asset), peaks);
    }

    /// Open a silent carrier so the metronome can tick when no song is loaded.
    pub fn open_click_carrier(&mut self) {
        if self.output.is_some() || self.carrier.is_some() || self.job.is_some() {
            return;
        }
        if self.asset.is_none() {
            let Ok(asset) = AudioAsset::from_interleaved(vec![0_i16; 48_000], 48_000, 1, 1.0)
            else {
                return;
            };
            let asset = Arc::new(asset);
            let peaks = PeakPyramid::build(&asset);
            self.asset = Some(asset);
            self.peaks = Some(peaks);
            self.bpm_guess = None;
            self.wave_cache = None;
        }
        let Some(asset) = self.asset.clone() else {
            return;
        };
        let generation = self.media_generation;
        self.carrier = Some(CarrierJob {
            generation,
            job: Job::spawn_typed(JobKind::AudioDecode, move |_| {
                AudioOutput::open_default(asset)
                    .map_err(|_| JobFailure::new(JobErrorCode::External))
            }),
        });
    }

    pub fn poll_click_carrier(&mut self, playing: bool, show_seconds: f32) {
        let Some(generation) = self.carrier.as_ref().map(|carrier| carrier.generation) else {
            return;
        };
        if generation != self.media_generation {
            if let Some(carrier) = &self.carrier {
                carrier.job.cancel();
            }
            self.carrier = None;
            return;
        }
        let Some(message) = self.carrier.as_mut().and_then(|carrier| carrier.job.poll()) else {
            return;
        };
        self.carrier = None;
        if let JobMsg::Done(output) = message {
            self.output = Some(output);
            self.last_click_config = None;
            if playing {
                self.seek_seconds(show_seconds);
                self.play();
            }
        }
    }

    /// Min/max columns for the visible count span. Empty when no song is loaded.
    /// A constant tempo uses one linear read. A tempo map samples each column
    /// in file time so a later tempo change stretches the wave.
    pub fn timeline_columns(
        &mut self,
        track: Option<&drill_core::audio::AudioTrack>,
        tempo: &drill_core::tempo::TempoMap,
        visible: std::ops::RangeInclusive<f32>,
        width: f32,
    ) -> Vec<(f32, f32)> {
        let Some(track) = track else {
            return Vec::new();
        };
        let Some(asset) = self.asset.clone() else {
            return Vec::new();
        };
        let Some(peaks) = self.peaks.clone() else {
            return Vec::new();
        };
        let buckets = waveform_bucket_count(width);
        let start = *visible.start();
        let end = *visible.end();
        let key = (
            start.to_bits(),
            (end - start).to_bits(),
            buckets as u32,
            track.offset_seconds.to_bits(),
            tempo_stamp(tempo),
        );
        if let Some(cache) = &self.wave_cache
            && cache.key == key
        {
            return cache.columns.clone();
        }
        let columns = if tempo.events().len() <= 1 {
            let t0 = drill_core::audio::count_to_audio_time(track, tempo, start)
                .clamp(0.0, track.duration_seconds);
            let t1 = drill_core::audio::count_to_audio_time(track, tempo, end)
                .clamp(t0, track.duration_seconds);
            let rate = asset.sample_rate() as f32;
            let frame0 = (t0 * rate).round().max(0.0) as u64;
            let frame1 = (t1 * rate).round().max(0.0) as u64;
            let mut scratch = Vec::new();
            peaks.range(
                &asset,
                frame0,
                frame1.max(frame0.saturating_add(1)),
                buckets,
                &mut scratch,
            );
            merge_channel_peaks(&scratch, asset.channels(), buckets)
        } else {
            let span = (end - start).max(f32::EPSILON);
            let windows = (0..buckets)
                .map(|index| {
                    let left = start + span * index as f32 / buckets as f32;
                    let right = start + span * (index + 1) as f32 / buckets as f32;
                    (
                        f64::from(drill_core::audio::count_to_audio_time(track, tempo, left)),
                        f64::from(drill_core::audio::count_to_audio_time(track, tempo, right)),
                    )
                })
                .collect::<Vec<_>>();
            peaks.column_envelope(&asset, &windows)
        };
        self.wave_cache = Some(WaveCache {
            key,
            columns: columns.clone(),
        });
        columns
    }

    pub fn configure_click(
        &mut self,
        tempo: &drill_core::tempo::TempoMap,
        end_count: f64,
        settings: &drill_audio::ClickSettings,
        file_origin_seconds: f64,
    ) -> usize {
        let rate = self.output.as_ref().map_or_else(
            || {
                self.asset
                    .as_ref()
                    .map_or(48_000, |asset| asset.sample_rate())
            },
            AudioOutput::output_sample_rate,
        );
        let origin = if file_origin_seconds.is_finite() {
            file_origin_seconds
        } else {
            0.0
        };
        // The full tempo map (not just its bpm at count 0) must be part of the
        // signature: a tempo change anchored anywhere after count 0 needs to
        // invalidate the cached click schedule too, or the click track can go
        // stale after a mid-show tempo edit. The file origin slides the clicks
        // onto the same timeline as the song.
        let signature = (*settings, end_count, tempo.clone(), rate, origin.to_bits());
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
        let shift = (origin * f64::from(rate)).round();
        if shift.is_finite() {
            self.click_schedule
                .shift_samples(shift.clamp(i64::MIN as f64, i64::MAX as f64) as i64);
        }
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

fn tempo_stamp(tempo: &drill_core::tempo::TempoMap) -> u64 {
    tempo.events().iter().fold(0u64, |hash, event| {
        hash.wrapping_mul(0x9E37_79B1)
            .wrapping_add(u64::from(event.count.to_bits()))
            .wrapping_mul(0x9E37_79B1)
            .wrapping_add(u64::from(event.bpm.to_bits()))
    })
}

fn merge_channel_peaks(peaks: &[Peak], channels: u8, buckets: usize) -> Vec<(f32, f32)> {
    let channels = usize::from(channels).max(1);
    let mut columns = Vec::with_capacity(buckets);
    for chunk in peaks.chunks(channels).take(buckets) {
        let peak = chunk.iter().copied().fold(Peak::SILENT, Peak::merge);
        columns.push((
            f32::from(peak.min) / 32_768.0,
            f32::from(peak.max) / 32_768.0,
        ));
    }
    columns
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
