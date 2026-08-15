use crate::clock::ClockShared;
use crate::{
    AudioAsset, ClickMixer, ClickSchedule, ClickSettings, ClickVoices, DuckingMode, PlaybackClock,
    PlaybackRate, RateError, render_block_rate_linear,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

pub const BLOCK_FRAMES: usize = 512;
pub const MAX_OUTPUT_CHANNELS: usize = 8;
pub const BLOCK_SAMPLES: usize = BLOCK_FRAMES * MAX_OUTPUT_CHANNELS;
const RING_BLOCKS: usize = 32;
const NO_SEEK: u64 = u64::MAX;

/// Read-only description of the current default output route. This probe does
/// not create or start a stream, so callers can safely use it in diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputDeviceInfo {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
}

/// Lock-free counters written by the realtime callback. Reading a snapshot is
/// safe from a diagnostics or UI thread and never changes stream behaviour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputDiagnostics {
    pub callbacks: u64,
    pub rendered_frames: u64,
    pub underruns: u64,
    pub device_errors: u64,
}

#[derive(Debug, Default)]
struct RealtimeDiagnostics {
    callbacks: AtomicU64,
    rendered_frames: AtomicU64,
    underruns: AtomicU64,
    device_errors: AtomicU64,
}

impl RealtimeDiagnostics {
    fn snapshot(&self) -> OutputDiagnostics {
        OutputDiagnostics {
            callbacks: self.callbacks.load(Ordering::Relaxed),
            rendered_frames: self.rendered_frames.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
            device_errors: self.device_errors.load(Ordering::Relaxed),
        }
    }
}

/// Probes the route CPAL would use for [`AudioOutput::open_default`]. A missing
/// device is returned as an error and must never be reported as a successful
/// test by callers.
pub fn probe_default_output() -> Result<OutputDeviceInfo, OutputError> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(OutputError::NoDefaultDevice)?;
    let name = device
        .description()
        .map(|description| description.to_string())
        .unwrap_or_else(|_| "Unknown output device".into());
    let supported = device
        .default_output_config()
        .map_err(|error| OutputError::DefaultConfig(error.to_string()))?;
    Ok(OutputDeviceInfo {
        name,
        sample_rate: supported.sample_rate(),
        channels: supported.channels(),
        sample_format: format!("{:?}", supported.sample_format()),
    })
}

#[derive(Clone)]
pub struct Block {
    generation: u32,
    start_sample: i64,
    source_start_q32: u64,
    source_step_q32: u64,
    frames: u16,
    channels: u8,
    data: [f32; BLOCK_SAMPLES],
}

impl Default for Block {
    fn default() -> Self {
        Self {
            generation: 0,
            start_sample: 0,
            source_start_q32: 0,
            source_step_q32: 1_u64 << 32,
            frames: 0,
            channels: 0,
            data: [0.0; BLOCK_SAMPLES],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MixerState {
    pub source_position_q32: u64,
    pub output_position: i64,
}

impl MixerState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            source_position_q32: 0,
            output_position: 0,
        }
    }

    pub fn seek_source_frame(&mut self, frame: u64, source_rate: u32, output_rate: u32) {
        self.source_position_q32 = frame.saturating_mul(1_u64 << 32);
        self.output_position = frame
            .saturating_mul(u64::from(output_rate))
            .checked_div(u64::from(source_rate).max(1))
            .unwrap_or(0)
            .min(i64::MAX as u64) as i64;
    }
}

impl Default for MixerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Device-independent PCM renderer. It performs no allocation or locking.
pub fn render_block(
    asset: &AudioAsset,
    output_rate: u32,
    output_channels: usize,
    gain: f32,
    muted: bool,
    state: &mut MixerState,
    out: &mut [f32],
) -> usize {
    if output_rate == 0 || output_channels == 0 || output_channels > MAX_OUTPUT_CHANNELS {
        out.fill(0.0);
        return 0;
    }
    let frames = (out.len() / output_channels).min(BLOCK_FRAMES);
    let source_channels = usize::from(asset.channels());
    let step_q32 = ((u64::from(asset.sample_rate()) << 32) / u64::from(output_rate)).max(1);
    for output_frame in 0..frames {
        let source_frame = state.source_position_q32 >> 32;
        let source = asset.frames_range(source_frame, 1);
        for channel in 0..output_channels {
            let value = if muted || source.is_empty() {
                0.0
            } else {
                let source_channel = channel.min(source_channels - 1);
                f32::from(source[source_channel]) / 32_768.0 * gain
            };
            out[output_frame * output_channels + channel] = value.clamp(-1.0, 1.0);
        }
        state.source_position_q32 = state.source_position_q32.saturating_add(step_q32);
    }
    out[frames * output_channels..].fill(0.0);
    state.output_position = state.output_position.saturating_add(frames as i64);
    frames
}

#[derive(Debug)]
pub enum OutputError {
    NoDefaultDevice,
    DefaultConfig(String),
    UnsupportedChannels(u16),
    BuildStream(String),
    StartStream(String),
    SpawnMixer(std::io::Error),
    ClickSampleRate { voices: u32, output: u32 },
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDefaultDevice => f.write_str("no default audio output device"),
            Self::DefaultConfig(error) => write!(f, "default output configuration failed: {error}"),
            Self::UnsupportedChannels(channels) => {
                write!(f, "unsupported output channel count: {channels}")
            }
            Self::BuildStream(error) => write!(f, "audio stream creation failed: {error}"),
            Self::StartStream(error) => write!(f, "audio stream start failed: {error}"),
            Self::SpawnMixer(error) => write!(f, "audio mixer thread creation failed: {error}"),
            Self::ClickSampleRate { voices, output } => write!(
                f,
                "click voice sample rate {voices} does not match output rate {output}"
            ),
        }
    }
}

#[derive(Clone, Debug)]
struct ClickProgram {
    generation: u32,
    settings: ClickSettings,
    schedule: ClickSchedule,
    voices: ClickVoices,
}

enum MixerCommand {
    SetClicks(ClickProgram),
    ClearClicks(u32),
}

fn apply_click_commands(
    receiver: &Receiver<MixerCommand>,
    current_generation: u32,
    timeline: i64,
    program: &mut Option<ClickProgram>,
    mixer: &mut ClickMixer,
    local_generation: &mut u32,
) {
    while let Ok(command) = receiver.try_recv() {
        match command {
            MixerCommand::SetClicks(next) if next.generation == current_generation => {
                *local_generation = next.generation;
                mixer.reset(&next.schedule, timeline);
                *program = Some(next);
            }
            MixerCommand::ClearClicks(generation) if generation == current_generation => {
                *local_generation = generation;
                *program = None;
                *mixer = ClickMixer::default();
            }
            // A later seek/range/config change already superseded this command.
            MixerCommand::SetClicks(_) | MixerCommand::ClearClicks(_) => {}
        }
    }
}

impl std::error::Error for OutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::SpawnMixer(error) => Some(error),
            _ => None,
        }
    }
}

struct Controls {
    playing: AtomicBool,
    muted: AtomicBool,
    gain_bits: AtomicU32,
    playback_rate_bits: AtomicU32,
    seek_source: AtomicU64,
    generation: AtomicU32,
    shutdown: AtomicBool,
}

pub struct AudioOutput {
    controls: Arc<Controls>,
    clock: PlaybackClock,
    stream: cpal::Stream,
    mixer: Option<JoinHandle<()>>,
    click_commands: Sender<MixerCommand>,
    output_rate: u32,
    diagnostics: Arc<RealtimeDiagnostics>,
}

impl AudioOutput {
    #[must_use]
    pub const fn output_sample_rate(&self) -> u32 {
        self.output_rate
    }

    pub fn open_default(asset: Arc<AudioAsset>) -> Result<Self, OutputError> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or(OutputError::NoDefaultDevice)?;
        let supported = device
            .default_output_config()
            .map_err(|error| OutputError::DefaultConfig(error.to_string()))?;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let channels = usize::from(config.channels);
        if channels == 0 || channels > MAX_OUTPUT_CHANNELS {
            return Err(OutputError::UnsupportedChannels(config.channels));
        }
        let sample_rate = config.sample_rate;
        let controls = Arc::new(Controls {
            playing: AtomicBool::new(false),
            muted: AtomicBool::new(false),
            gain_bits: AtomicU32::new(1.0_f32.to_bits()),
            playback_rate_bits: AtomicU32::new(PlaybackRate::NORMAL.get().to_bits()),
            seek_source: AtomicU64::new(NO_SEEK),
            generation: AtomicU32::new(0),
            shutdown: AtomicBool::new(false),
        });
        let clock_shared = Arc::new(ClockShared::new(sample_rate));
        let clock = PlaybackClock::new(Arc::clone(&clock_shared));
        let diagnostics = Arc::new(RealtimeDiagnostics::default());
        let (producer, consumer) = rtrb::RingBuffer::<Block>::new(RING_BLOCKS);
        let (click_commands, click_receiver) = std::sync::mpsc::channel();
        let mixer_controls = Arc::clone(&controls);
        let mixer = std::thread::Builder::new()
            .name("drill-audio-mixer".into())
            .spawn(move || {
                mixer_loop(
                    asset,
                    sample_rate,
                    channels,
                    mixer_controls,
                    producer,
                    click_receiver,
                );
            })
            .map_err(OutputError::SpawnMixer)?;

        let callback_controls = Arc::clone(&controls);
        let stream = match sample_format {
            cpal::SampleFormat::F32 => build_stream::<f32>(
                &device,
                &config,
                consumer,
                callback_controls,
                clock_shared,
                Arc::clone(&diagnostics),
            ),
            cpal::SampleFormat::I16 => build_stream::<i16>(
                &device,
                &config,
                consumer,
                callback_controls,
                clock_shared,
                Arc::clone(&diagnostics),
            ),
            cpal::SampleFormat::U16 => build_stream::<u16>(
                &device,
                &config,
                consumer,
                callback_controls,
                clock_shared,
                Arc::clone(&diagnostics),
            ),
            other => Err(OutputError::BuildStream(format!(
                "unsupported device sample format: {other:?}"
            ))),
        }?;
        stream
            .play()
            .map_err(|error| OutputError::StartStream(error.to_string()))?;
        Ok(Self {
            controls,
            clock,
            stream,
            mixer: Some(mixer),
            click_commands,
            output_rate: sample_rate,
            diagnostics,
        })
    }

    pub fn play(&self) {
        self.controls.playing.store(true, Ordering::Release);
    }

    pub fn pause(&self) {
        self.controls.playing.store(false, Ordering::Release);
    }

    pub fn seek(&self, source_frame: u64) {
        self.controls
            .seek_source
            .store(source_frame, Ordering::Release);
        self.controls.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn set_gain(&self, gain: f32) {
        let gain = if gain.is_finite() {
            gain.clamp(0.0, 4.0)
        } else {
            1.0
        };
        self.controls
            .gain_bits
            .store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn set_muted(&self, muted: bool) {
        self.controls.muted.store(muted, Ordering::Relaxed);
    }

    pub fn set_playback_rate(&self, rate: f32) -> Result<(), RateError> {
        let rate = PlaybackRate::new(rate)?;
        self.controls
            .playback_rate_bits
            .store(rate.get().to_bits(), Ordering::Release);
        Ok(())
    }

    /// Replaces the click/count-in program. Schedule and voices should be
    /// prepared off the realtime thread. Old queued audio is rejected by its
    /// generation before it can reach the device callback.
    pub fn set_clicks(
        &self,
        settings: ClickSettings,
        schedule: ClickSchedule,
        voices: ClickVoices,
    ) -> Result<(), OutputError> {
        if voices.sample_rate() != self.output_rate {
            return Err(OutputError::ClickSampleRate {
                voices: voices.sample_rate(),
                output: self.output_rate,
            });
        }
        let generation = self.controls.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let _ = self
            .click_commands
            .send(MixerCommand::SetClicks(ClickProgram {
                generation,
                settings,
                schedule,
                voices,
            }));
        Ok(())
    }

    /// Disables clicks and invalidates all already queued click-mixed blocks.
    pub fn clear_clicks(&self) {
        let generation = self.controls.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let _ = self
            .click_commands
            .send(MixerCommand::ClearClicks(generation));
    }

    #[must_use]
    pub fn clock(&self) -> PlaybackClock {
        self.clock.clone()
    }

    /// Returns a coherent-enough monotonic counter snapshot. Individual
    /// fields may be from adjacent callbacks, which is intentional: all are
    /// independent totals and never require a callback-side lock.
    #[must_use]
    pub fn diagnostics(&self) -> OutputDiagnostics {
        self.diagnostics.snapshot()
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        self.controls.shutdown.store(true, Ordering::Release);
        self.controls.playing.store(false, Ordering::Release);
        let _ = self.stream.pause();
        if let Some(mixer) = self.mixer.take() {
            let _ = mixer.join();
        }
    }
}

fn mixer_loop(
    asset: Arc<AudioAsset>,
    sample_rate: u32,
    channels: usize,
    controls: Arc<Controls>,
    mut producer: rtrb::Producer<Block>,
    click_receiver: Receiver<MixerCommand>,
) {
    let mut state = MixerState::new();
    let mut click_program: Option<ClickProgram> = None;
    let mut click_mixer = ClickMixer::default();
    let mut local_generation = controls.generation.load(Ordering::Acquire);
    while !controls.shutdown.load(Ordering::Acquire) {
        apply_click_commands(
            &click_receiver,
            controls.generation.load(Ordering::Acquire),
            state.output_position,
            &mut click_program,
            &mut click_mixer,
            &mut local_generation,
        );
        let seek = controls.seek_source.swap(NO_SEEK, Ordering::AcqRel);
        if seek != NO_SEEK {
            state.seek_source_frame(seek, asset.sample_rate(), sample_rate);
            local_generation = controls.generation.load(Ordering::Acquire);
            if let Some(program) = &click_program {
                click_mixer.reset(&program.schedule, state.output_position);
            }
        }
        let mut block = Block {
            generation: local_generation,
            start_sample: state.output_position,
            source_start_q32: state.source_position_q32,
            channels: channels as u8,
            ..Block::default()
        };
        let rate = PlaybackRate::new(f32::from_bits(
            controls.playback_rate_bits.load(Ordering::Acquire),
        ))
        .unwrap_or(PlaybackRate::NORMAL);
        let source_before = state.source_position_q32;
        let timeline = state.output_position;
        let track_gain = click_program.as_ref().map_or(1.0, |program| {
            let duck = match program.settings.ducking {
                DuckingMode::Always => true,
                DuckingMode::CountInOnly => timeline < 0,
            };
            if duck {
                program.settings.duck_gain.clamp(0.0, 1.0)
            } else {
                1.0
            }
        });
        let frames = render_block_rate_linear(
            &asset,
            sample_rate,
            channels,
            rate,
            None,
            track_gain,
            false,
            &mut state,
            &mut block.data,
        );
        if let Some(program) = &click_program
            && program.settings.enabled
        {
            click_mixer.add_block(
                &program.schedule,
                &program.voices,
                timeline,
                channels,
                &mut block.data[..frames * channels],
            );
        }
        block.source_step_q32 =
            state.source_position_q32.saturating_sub(source_before) / frames.max(1) as u64;
        block.frames = frames as u16;
        match producer.push(block) {
            Ok(()) => {}
            Err(rtrb::PushError::Full(_)) => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: rtrb::Consumer<Block>,
    controls: Arc<Controls>,
    clock: Arc<ClockShared>,
    diagnostics: Arc<RealtimeDiagnostics>,
) -> Result<cpal::Stream, OutputError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let sample_rate = config.sample_rate;
    let channels = usize::from(config.channels);
    let mut current = Block::default();
    let mut cursor = 0_usize;
    let callback_diagnostics = Arc::clone(&diagnostics);
    device
        .build_output_stream(
            *config,
            move |output: &mut [T], _| {
                callback_diagnostics
                    .callbacks
                    .fetch_add(1, Ordering::Relaxed);
                callback_diagnostics
                    .rendered_frames
                    .fetch_add((output.len() / channels) as u64, Ordering::Relaxed);
                if !controls.playing.load(Ordering::Acquire) {
                    output.fill(T::from_sample(0.0));
                    clock.write(
                        current
                            .start_sample
                            .saturating_add((cursor / channels) as i64),
                        current.source_start_q32.saturating_add(
                            current
                                .source_step_q32
                                .saturating_mul((cursor / channels) as u64),
                        ),
                        sample_rate,
                        false,
                    );
                    return;
                }
                let mut written = 0;
                while written < output.len() {
                    if cursor >= usize::from(current.frames) * channels {
                        loop {
                            match consumer.pop() {
                                Ok(block)
                                    if block.generation
                                        == controls.generation.load(Ordering::Acquire) =>
                                {
                                    if usize::from(block.channels) != channels {
                                        continue;
                                    }
                                    current = block;
                                    cursor = 0;
                                    break;
                                }
                                Ok(_) => continue,
                                Err(rtrb::PopError::Empty) => {
                                    callback_diagnostics
                                        .underruns
                                        .fetch_add(1, Ordering::Relaxed);
                                    output[written..].fill(T::from_sample(0.0));
                                    clock.write(
                                        current.start_sample,
                                        current.source_start_q32,
                                        sample_rate,
                                        true,
                                    );
                                    return;
                                }
                            }
                        }
                    }
                    let available = usize::from(current.frames) * channels - cursor;
                    let count = available.min(output.len() - written);
                    let muted = controls.muted.load(Ordering::Relaxed);
                    let gain = f32::from_bits(controls.gain_bits.load(Ordering::Relaxed));
                    for (destination, source) in output[written..written + count]
                        .iter_mut()
                        .zip(&current.data[cursor..cursor + count])
                    {
                        *destination = T::from_sample(if muted {
                            0.0
                        } else {
                            (*source * gain).clamp(-1.0, 1.0)
                        });
                    }
                    cursor += count;
                    written += count;
                    let played_frames = cursor / channels;
                    clock.write(
                        current.start_sample.saturating_add(played_frames as i64),
                        current.source_start_q32.saturating_add(
                            current.source_step_q32.saturating_mul(played_frames as u64),
                        ),
                        sample_rate,
                        true,
                    );
                }
            },
            move |_error| {
                // The realtime callback cannot log, allocate, or lock. Device
                // fault reporting is a fixed-size atomic counter only.
                diagnostics.device_errors.fetch_add(1, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|error| OutputError::BuildStream(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use drill_core::tempo::TempoMap;

    #[test]
    fn mixer_renders_gain_mute_seek_and_rate_conversion() {
        let asset =
            AudioAsset::from_interleaved(vec![16_384, -16_384, 8_192, -8_192], 2, 2, 1.0).unwrap();
        let mut state = MixerState::new();
        let mut output = [0.0; 8];
        assert_eq!(
            render_block(&asset, 2, 2, 0.5, false, &mut state, &mut output),
            4
        );
        assert_eq!(&output[..4], &[0.25, -0.25, 0.125, -0.125]);
        assert_eq!(&output[4..], &[0.0; 4]);
        state.seek_source_frame(1, 2, 4);
        render_block(&asset, 4, 2, 1.0, true, &mut state, &mut output);
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert_eq!(state.output_position, 6);
    }

    #[test]
    fn ring_boundary_preserves_blocks_without_locks() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::new(2);
        let block = Block {
            frames: 1,
            data: [0.25; BLOCK_SAMPLES],
            ..Block::default()
        };
        producer.push(block).unwrap();
        let received = consumer.pop().unwrap();
        assert_eq!(received.frames, 1);
        assert_eq!(received.data[0], 0.25);
    }

    #[test]
    fn stale_click_program_is_discarded_by_generation() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let settings = ClickSettings {
            enabled: true,
            ..ClickSettings::default()
        };
        sender
            .send(MixerCommand::SetClicks(ClickProgram {
                generation: 4,
                settings,
                schedule: ClickSchedule::build(
                    &TempoMap::constant(120.0),
                    0.0,
                    8.0,
                    &settings,
                    48_000,
                ),
                voices: ClickVoices::render(&settings, 48_000),
            }))
            .unwrap();
        let mut program = None;
        let mut mixer = ClickMixer::default();
        let mut generation = 5;

        apply_click_commands(&receiver, 5, 0, &mut program, &mut mixer, &mut generation);

        assert!(program.is_none());
        assert_eq!(generation, 5);
    }

    #[test]
    fn newest_click_program_updates_generation_without_device() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let settings = ClickSettings {
            enabled: true,
            ..ClickSettings::default()
        };
        sender
            .send(MixerCommand::SetClicks(ClickProgram {
                generation: 9,
                settings,
                schedule: ClickSchedule::default(),
                voices: ClickVoices::render(&settings, 48_000),
            }))
            .unwrap();
        let mut program = None;
        let mut mixer = ClickMixer::default();
        let mut generation = 8;
        apply_click_commands(&receiver, 9, 512, &mut program, &mut mixer, &mut generation);
        assert!(program.is_some());
        assert_eq!(generation, 9);
    }
}
