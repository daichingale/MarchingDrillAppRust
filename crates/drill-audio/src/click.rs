use crate::{AudioAsset, MixerState, render_block};
use drill_core::tempo::TempoMap;
use std::f32::consts::TAU;

const MAX_ACTIVE_VOICES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickMode {
    BeforeStartOnly,
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuckingMode {
    CountInOnly,
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum CountIn {
    Two = 2,
    Four = 4,
    Eight = 8,
    Sixteen = 16,
}

impl CountIn {
    #[must_use]
    pub const fn counts(self) -> u16 {
        self as u16
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickSettings {
    pub enabled: bool,
    pub accent_hz: f32,
    pub beat_hz: f32,
    pub decay_seconds: f32,
    pub volume: f32,
    pub beats_per_measure: u16,
    /// Clicks per count. Sanitized to one of 1, 2, 4, or 8.
    pub subdivision: u8,
    pub count_in: CountIn,
    pub mode: ClickMode,
    pub ducking: DuckingMode,
    /// Reference-track multiplier while ducking is active.
    pub duck_gain: f32,
}

impl Default for ClickSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            accent_hz: 1_600.0,
            beat_hz: 1_000.0,
            decay_seconds: 0.030,
            volume: 0.5,
            beats_per_measure: 4,
            subdivision: 1,
            count_in: CountIn::Eight,
            mode: ClickMode::Always,
            ducking: DuckingMode::CountInOnly,
            duck_gain: 0.25,
        }
    }
}

impl ClickSettings {
    fn sanitized(self, sample_rate: u32) -> Self {
        Self {
            accent_hz: finite_clamp(self.accent_hz, 20.0, sample_rate as f32 * 0.45, 1_600.0),
            beat_hz: finite_clamp(self.beat_hz, 20.0, sample_rate as f32 * 0.45, 1_000.0),
            decay_seconds: finite_clamp(self.decay_seconds, 0.001, 0.5, 0.030),
            volume: finite_clamp(self.volume, 0.0, 2.0, 0.5),
            beats_per_measure: self.beats_per_measure.max(1),
            subdivision: match self.subdivision {
                1 | 2 | 4 | 8 => self.subdivision,
                _ => 1,
            },
            duck_gain: finite_clamp(self.duck_gain, 0.0, 1.0, 0.25),
            ..self
        }
    }
}

fn finite_clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max.max(min))
    } else {
        fallback.clamp(min, max.max(min))
    }
}

#[derive(Clone, Debug)]
pub struct ClickVoices {
    accent: Box<[f32]>,
    beat: Box<[f32]>,
    rate: u32,
}

impl ClickVoices {
    #[must_use]
    pub fn render(settings: &ClickSettings, sample_rate: u32) -> Self {
        let sample_rate = sample_rate.max(1);
        let settings = settings.sanitized(sample_rate);
        Self {
            accent: render_voice(settings.accent_hz, &settings, sample_rate),
            beat: render_voice(settings.beat_hz, &settings, sample_rate),
            rate: sample_rate,
        }
    }

    #[must_use]
    pub fn voice(&self, accent: bool) -> &[f32] {
        if accent { &self.accent } else { &self.beat }
    }

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.rate
    }
}

fn render_voice(frequency: f32, settings: &ClickSettings, sample_rate: u32) -> Box<[f32]> {
    let length = (4.0 * settings.decay_seconds * sample_rate as f32).ceil() as usize;
    let mut voice = Vec::with_capacity(length.max(1));
    for index in 0..length.max(1) {
        let time = index as f32 / sample_rate as f32;
        let envelope = (-time / settings.decay_seconds).exp();
        let fade = if length.saturating_sub(index) <= 32 {
            let remaining = length.saturating_sub(index) as f32;
            0.5 - 0.5 * (std::f32::consts::PI * remaining / 32.0).cos()
        } else {
            1.0
        };
        voice.push((TAU * frequency * time).sin() * envelope * fade * settings.volume);
    }
    if let Some(last) = voice.last_mut() {
        *last = 0.0;
    }
    voice.into_boxed_slice()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClickEvent {
    /// Sample index relative to the selected playback-range start.
    pub timeline: i64,
    pub accent: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ClickSchedule {
    events: Vec<ClickEvent>,
}

impl ClickSchedule {
    #[must_use]
    pub fn build(
        tempo: &TempoMap,
        range_start: f64,
        range_end: f64,
        settings: &ClickSettings,
        sample_rate: u32,
    ) -> Self {
        let mut schedule = Self::default();
        Self::build_into(
            tempo,
            range_start,
            range_end,
            settings,
            sample_rate,
            &mut schedule,
        );
        schedule
    }

    pub fn build_into(
        tempo: &TempoMap,
        range_start: f64,
        range_end: f64,
        settings: &ClickSettings,
        sample_rate: u32,
        out: &mut Self,
    ) {
        out.events.clear();
        if sample_rate == 0
            || !range_start.is_finite()
            || !range_end.is_finite()
            || range_end <= range_start
        {
            return;
        }
        let settings = settings.sanitized(sample_rate);
        let subdivision = f64::from(settings.subdivision);
        let first_tick = (range_start * subdivision).ceil() as i64;
        let last_tick = (range_end * subdivision).ceil() as i64;
        let origin_seconds = tempo.seconds_at_f64(range_start);
        let count_in = i64::from(settings.count_in.counts());
        let count_seconds = 60.0 / f64::from(tempo.bpm_at(range_start as f32));
        for before in (1..=count_in).rev() {
            out.events.push(ClickEvent {
                timeline: (-(before as f64) * count_seconds * f64::from(sample_rate)).round()
                    as i64,
                accent: ((first_tick / i64::from(settings.subdivision)) - before)
                    .rem_euclid(i64::from(settings.beats_per_measure))
                    == 0,
            });
        }
        if settings.mode == ClickMode::Always {
            for tick in first_tick..last_tick {
                let count = tick as f64 / subdivision;
                let timeline = ((tempo.seconds_at_f64(count) - origin_seconds)
                    * f64::from(sample_rate))
                .round() as i64;
                let on_count = tick.rem_euclid(i64::from(settings.subdivision)) == 0;
                let count_index = tick.div_euclid(i64::from(settings.subdivision));
                out.events.push(ClickEvent {
                    timeline,
                    accent: on_count
                        && count_index.rem_euclid(i64::from(settings.beats_per_measure)) == 0,
                });
            }
        }
        out.events.sort_by_key(|event| event.timeline);
    }

    #[must_use]
    pub fn lower_bound(&self, from: i64) -> usize {
        self.events.partition_point(|event| event.timeline < from)
    }

    #[must_use]
    pub fn events(&self) -> &[ClickEvent] {
        &self.events
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct ActiveVoice {
    event_index: usize,
    offset: usize,
    active: bool,
}

#[derive(Clone, Debug)]
pub struct ClickMixer {
    active: [ActiveVoice; MAX_ACTIVE_VOICES],
    next_event: usize,
    expected_timeline: i64,
}

impl Default for ClickMixer {
    fn default() -> Self {
        Self {
            active: [ActiveVoice::default(); MAX_ACTIVE_VOICES],
            next_event: 0,
            expected_timeline: i64::MIN,
        }
    }
}

impl ClickMixer {
    pub fn reset(&mut self, schedule: &ClickSchedule, timeline: i64) {
        self.active.fill(ActiveVoice::default());
        self.next_event = schedule.lower_bound(timeline);
        self.expected_timeline = timeline;
    }

    /// Adds clicks in-place without allocating.
    pub fn add_block(
        &mut self,
        schedule: &ClickSchedule,
        voices: &ClickVoices,
        timeline: i64,
        channels: usize,
        output: &mut [f32],
    ) {
        if channels == 0 {
            return;
        }
        let frames = output.len() / channels;
        if timeline != self.expected_timeline {
            self.reset(schedule, timeline);
        }
        let block_end = timeline.saturating_add(frames as i64);
        while let Some(event) = schedule.events.get(self.next_event)
            && event.timeline < block_end
        {
            if event.timeline >= timeline {
                let slot = self
                    .active
                    .iter()
                    .position(|voice| !voice.active)
                    .unwrap_or(0);
                self.active[slot] = ActiveVoice {
                    event_index: self.next_event,
                    offset: 0,
                    active: true,
                };
            }
            self.next_event += 1;
        }
        for voice in &mut self.active {
            if !voice.active {
                continue;
            }
            let event = schedule.events[voice.event_index];
            let table = voices.voice(event.accent);
            let start_frame = event.timeline.saturating_sub(timeline).max(0) as usize;
            for frame in start_frame..frames {
                let Some(sample) = table.get(voice.offset).copied() else {
                    voice.active = false;
                    break;
                };
                for channel in 0..channels {
                    let index = frame * channels + channel;
                    output[index] = (output[index] + sample).clamp(-1.0, 1.0);
                }
                voice.offset += 1;
            }
        }
        self.expected_timeline = block_end;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render_mix_with_clicks(
    asset: &AudioAsset,
    output_rate: u32,
    output_channels: usize,
    track_gain: f32,
    track_muted: bool,
    timeline: i64,
    settings: &ClickSettings,
    schedule: &ClickSchedule,
    voices: &ClickVoices,
    mixer: &mut MixerState,
    click_mixer: &mut ClickMixer,
    out: &mut [f32],
) -> usize {
    let duck = match settings.ducking {
        DuckingMode::Always => true,
        DuckingMode::CountInOnly => timeline < 0,
    };
    let gain = track_gain
        * if duck {
            settings.duck_gain.clamp(0.0, 1.0)
        } else {
            1.0
        };
    let frames = render_block(
        asset,
        output_rate,
        output_channels,
        gain,
        track_muted,
        mixer,
        out,
    );
    if settings.enabled {
        click_mixer.add_block(schedule, voices, timeline, output_channels, out);
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variable_tempo_schedule_uses_exact_phase_at_arbitrary_start() {
        let mut tempo = TempoMap::constant(120.0);
        tempo.set(10.0, 60.0);
        let settings = ClickSettings {
            subdivision: 2,
            ..ClickSettings::default()
        };
        let schedule = ClickSchedule::build(&tempo, 8.0, 12.0, &settings, 48_000);
        let at_ten = schedule
            .events()
            .iter()
            .find(|event| !event.accent && event.timeline == 48_000)
            .unwrap();
        assert_eq!(at_ten.timeline, 48_000);
        assert!(
            schedule
                .events()
                .windows(2)
                .all(|pair| pair[0].timeline <= pair[1].timeline)
        );
    }

    #[test]
    fn count_in_options_land_before_zero() {
        for count_in in [
            CountIn::Two,
            CountIn::Four,
            CountIn::Eight,
            CountIn::Sixteen,
        ] {
            let settings = ClickSettings {
                count_in,
                mode: ClickMode::BeforeStartOnly,
                ..ClickSettings::default()
            };
            let schedule =
                ClickSchedule::build(&TempoMap::constant(120.0), 32.0, 40.0, &settings, 48_000);
            assert_eq!(schedule.events().len(), usize::from(count_in.counts()));
            assert!(schedule.events().iter().all(|event| event.timeline < 0));
            assert_eq!(schedule.events().last().unwrap().timeline, -24_000);
        }
    }

    #[test]
    fn click_mixer_continues_voice_across_blocks_without_allocation() {
        let settings = ClickSettings {
            enabled: true,
            count_in: CountIn::Two,
            mode: ClickMode::Always,
            ..ClickSettings::default()
        };
        let schedule =
            ClickSchedule::build(&TempoMap::constant(120.0), 0.0, 2.0, &settings, 48_000);
        let voices = ClickVoices::render(&settings, 48_000);
        let mut mixer = ClickMixer::default();
        let mut first = [0.0; 256];
        let mut second = [0.0; 256];
        mixer.add_block(&schedule, &voices, 0, 1, &mut first);
        mixer.add_block(&schedule, &voices, 256, 1, &mut second);
        assert!(first.iter().any(|sample| *sample != 0.0));
        assert!(second.iter().any(|sample| *sample != 0.0));
    }

    #[test]
    fn ducking_reduces_track_during_count_in() {
        let asset = AudioAsset::from_interleaved(vec![16_384; 32], 48_000, 1, 1.0).unwrap();
        let settings = ClickSettings {
            enabled: false,
            duck_gain: 0.25,
            ..ClickSettings::default()
        };
        let schedule = ClickSchedule::default();
        let voices = ClickVoices::render(&settings, 48_000);
        let mut mixer = MixerState::new();
        let mut click_mixer = ClickMixer::default();
        let mut output = [0.0; 8];
        render_mix_with_clicks(
            &asset,
            48_000,
            1,
            1.0,
            false,
            -8,
            &settings,
            &schedule,
            &voices,
            &mut mixer,
            &mut click_mixer,
            &mut output,
        );
        assert!(output.iter().all(|sample| (*sample - 0.125).abs() < 1e-6));
    }
}
