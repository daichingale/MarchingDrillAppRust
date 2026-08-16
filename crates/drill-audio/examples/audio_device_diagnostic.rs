use drill_audio::{
    AudioAsset, AudioOutput, ClickSchedule, ClickSettings, ClickVoices, OutputDeviceInfo,
    OutputDiagnostics, OutputError, probe_default_output,
};
use drill_core::tempo::TempoMap;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const EXIT_NO_DEVICE: u8 = 77;
const EXIT_BAD_ARGUMENTS: u8 = 64;
const MAX_DURATION_SECONDS: f64 = 7_200.0;
const MAX_REPORT_SAMPLES: usize = 512;

#[derive(Debug)]
struct Args {
    duration: Duration,
    report: Option<PathBuf>,
    mock: bool,
    audible: bool,
}

#[derive(Clone, Debug, Serialize)]
struct DeviceReport {
    name: String,
    sample_rate: u32,
    channels: u16,
    sample_format: String,
}

impl From<OutputDeviceInfo> for DeviceReport {
    fn from(value: OutputDeviceInfo) -> Self {
        Self {
            name: value.name,
            sample_rate: value.sample_rate,
            channels: value.channels,
            sample_format: value.sample_format,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct Sample {
    elapsed_ms: u64,
    position: i64,
    source_frame: u64,
    callbacks: u64,
    underruns: u64,
    device_errors: u64,
    rss_bytes: Option<u64>,
    private_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
struct Report {
    schema: &'static str,
    started_unix_ms: u128,
    requested_duration_ms: u64,
    actual_duration_ms: u64,
    mode: &'static str,
    device: DeviceReport,
    passed: bool,
    callbacks: u64,
    rendered_frames: u64,
    underruns: u64,
    callback_stalls: u64,
    clock_regressions: u64,
    device_errors: u64,
    reopen_attempts: u64,
    reopen_successes: u64,
    seeks: u64,
    rate_changes: u64,
    click_program_changes: u64,
    rss_start_bytes: Option<u64>,
    rss_peak_bytes: Option<u64>,
    rss_end_bytes: Option<u64>,
    rss_growth_bytes: Option<i64>,
    private_start_bytes: Option<u64>,
    private_peak_bytes: Option<u64>,
    private_end_bytes: Option<u64>,
    samples_truncated: bool,
    samples: Vec<Sample>,
    failures: Vec<String>,
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("argument error: {error}");
            return ExitCode::from(EXIT_BAD_ARGUMENTS);
        }
    };
    let result = if args.mock {
        run_mock(&args)
    } else {
        run_device(&args)
    };
    let report = match result {
        Ok(report) => report,
        Err(OutputError::NoDefaultDevice) => {
            eprintln!("SKIP: no default audio output device is available");
            return ExitCode::from(EXIT_NO_DEVICE);
        }
        Err(error) => {
            eprintln!("FAIL: audio device endurance setup failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let json = serde_json::to_string_pretty(&report).expect("bounded report serializes");
    if let Some(path) = &args.report {
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            eprintln!("FAIL: cannot create report directory: {error}");
            return ExitCode::FAILURE;
        }
        if let Err(error) = fs::write(path, format!("{json}\n")) {
            eprintln!("FAIL: cannot write report: {error}");
            return ExitCode::FAILURE;
        }
        println!("REPORT: {}", path.display());
    } else {
        println!("{json}");
    }
    if report.passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut duration = Duration::from_secs(5);
    let mut report = None;
    let mut mock = false;
    let mut audible = false;
    let mut args = args.peekable();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--duration-seconds" => {
                let value = args.next().ok_or("--duration-seconds needs a value")?;
                let seconds: f64 = value.parse().map_err(|_| "invalid duration")?;
                if !seconds.is_finite() || !(0.25..=MAX_DURATION_SECONDS).contains(&seconds) {
                    return Err(format!("duration must be 0.25..={MAX_DURATION_SECONDS} seconds"));
                }
                duration = Duration::from_secs_f64(seconds);
            }
            "--report" => report = Some(PathBuf::from(args.next().ok_or("--report needs a path")?)),
            "--mock" => mock = true,
            "--audible" => audible = true,
            "--help" | "-h" => return Err("usage: audio_device_diagnostic [--duration-seconds N] [--report FILE] [--mock] [--audible]".into()),
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(Args {
        duration,
        report,
        mock,
        audible,
    })
}

fn run_device(args: &Args) -> Result<Report, OutputError> {
    let device = probe_default_output()?;
    let frames = usize::try_from(device.sample_rate)
        .unwrap_or(48_000)
        .saturating_mul(8);
    let mut pcm = vec![0_i16; frames];
    if args.audible {
        for (index, sample) in pcm.iter_mut().enumerate() {
            let phase = index as f32 * 440.0 * std::f32::consts::TAU / device.sample_rate as f32;
            *sample = (phase.sin() * 1_000.0) as i16;
        }
    }
    let asset = Arc::new(
        AudioAsset::from_interleaved(pcm, device.sample_rate, 1, 1.0)
            .expect("valid generated asset"),
    );
    let mut output = Some(AudioOutput::open_default(Arc::clone(&asset))?);
    output.as_ref().unwrap().set_muted(!args.audible);
    output.as_ref().unwrap().play();
    let started_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let start = Instant::now();
    let sample_interval = args
        .duration
        .div_f64((MAX_REPORT_SAMPLES - 1) as f64)
        .max(Duration::from_millis(250));
    let mut next_sample = Duration::ZERO;
    let mut next_stress = Duration::from_millis(500);
    let mut last_position = 0;
    let mut last_callbacks = 0;
    let mut last_advance = Instant::now();
    let mut pending_seek: Option<u64> = None;
    let mut samples = Vec::with_capacity(MAX_REPORT_SAMPLES);
    let mut failures = Vec::new();
    let mut stalls = 0;
    let mut regressions = 0;
    let mut seeks = 0;
    let mut rate_changes = 0;
    let mut click_changes = 0;
    let mut reopen_attempts = 0;
    let mut reopen_successes = 0;
    let rates = [0.5_f32, 1.0, 1.5, 2.0, 0.75, 1.25];
    let mut stress_index = 0_usize;
    while start.elapsed() < args.duration {
        thread::sleep(Duration::from_millis(25));
        let elapsed = start.elapsed();
        if elapsed >= next_stress {
            let seek_target = ((stress_index * 7_919) % frames) as u64;
            output.as_ref().unwrap().seek(seek_target);
            pending_seek = Some(seek_target);
            seeks += 1;
            let _ = output
                .as_ref()
                .unwrap()
                .set_playback_rate(rates[stress_index % rates.len()]);
            rate_changes += 1;
            let settings = ClickSettings {
                enabled: true,
                subdivision: [1, 2, 4, 8][stress_index % 4],
                ..ClickSettings::default()
            };
            let schedule = ClickSchedule::build(
                &TempoMap::constant(120.0),
                0.0,
                32.0,
                &settings,
                device.sample_rate,
            );
            output.as_ref().unwrap().set_clicks(
                settings,
                schedule,
                ClickVoices::render(&settings, device.sample_rate),
            )?;
            click_changes += 1;
            stress_index += 1;
            next_stress += Duration::from_millis(500);
        }
        let clock = output.as_ref().unwrap().clock().sample();
        let diagnostics = output.as_ref().unwrap().diagnostics();
        if diagnostics.callbacks > last_callbacks || clock.position != last_position {
            last_advance = Instant::now();
        }
        let source_frame = clock.source_position_q32 >> 32;
        let seek_applied = pending_seek.is_some_and(|target| {
            source_frame.abs_diff(target) <= u64::from(device.sample_rate) / 4
        });
        if seek_applied {
            pending_seek = None;
        } else if clock.position < last_position && pending_seek.is_none() {
            regressions += 1;
        }
        last_position = clock.position;
        last_callbacks = diagnostics.callbacks;
        if last_advance.elapsed() > Duration::from_secs(2) {
            stalls += 1;
            last_advance = Instant::now();
            reopen_attempts += 1;
            drop(output.take());
            output = Some(match AudioOutput::open_default(Arc::clone(&asset)) {
                Ok(reopened) => {
                    reopen_successes += 1;
                    pending_seek = Some(0);
                    reopened
                }
                Err(error) => {
                    failures.push(format!("reopen failed: {error}"));
                    break;
                }
            });
            output.as_ref().unwrap().set_muted(!args.audible);
            output.as_ref().unwrap().play();
        }
        if elapsed >= next_sample && samples.len() < MAX_REPORT_SAMPLES {
            let memory = process_memory();
            samples.push(make_sample(elapsed, clock, diagnostics, memory));
            next_sample += sample_interval;
        }
    }
    let output = output.expect("output exists after completed run");
    output.pause();
    let final_diagnostics = output.diagnostics();
    if final_diagnostics.callbacks == 0 {
        failures.push("no output callback was observed".into());
    }
    if regressions > 0 {
        failures.push(format!(
            "sample clock regressed {regressions} times outside seek epochs"
        ));
    }
    if stalls > 0 {
        failures.push(format!("callback stalled {stalls} times"));
    }
    if final_diagnostics.device_errors > 0 {
        failures.push(format!(
            "device callback reported {} errors",
            final_diagnostics.device_errors
        ));
    }
    Ok(finish_report(
        started_unix_ms,
        args,
        "device",
        device.into(),
        final_diagnostics,
        stalls,
        regressions,
        reopen_attempts,
        reopen_successes,
        seeks,
        rate_changes,
        click_changes,
        samples,
        failures,
    ))
}

fn run_mock(args: &Args) -> Result<Report, OutputError> {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let memory = process_memory();
    let samples = vec![Sample {
        elapsed_ms: 0,
        position: 0,
        source_frame: 0,
        callbacks: 1,
        underruns: 0,
        device_errors: 0,
        rss_bytes: memory.0,
        private_bytes: memory.1,
    }];
    Ok(finish_report(
        started,
        args,
        "mock",
        DeviceReport {
            name: "deterministic-mock".into(),
            sample_rate: 48_000,
            channels: 2,
            sample_format: "F32".into(),
        },
        OutputDiagnostics {
            callbacks: 1,
            rendered_frames: 512,
            underruns: 0,
            device_errors: 0,
        },
        0,
        0,
        0,
        0,
        1,
        1,
        1,
        samples,
        Vec::new(),
    ))
}

fn make_sample(
    elapsed: Duration,
    clock: drill_audio::ClockSample,
    diagnostics: OutputDiagnostics,
    memory: (Option<u64>, Option<u64>),
) -> Sample {
    Sample {
        elapsed_ms: elapsed.as_millis().min(u128::from(u64::MAX)) as u64,
        position: clock.position,
        source_frame: clock.source_position_q32 >> 32,
        callbacks: diagnostics.callbacks,
        underruns: diagnostics.underruns,
        device_errors: diagnostics.device_errors,
        rss_bytes: memory.0,
        private_bytes: memory.1,
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_report(
    started_unix_ms: u128,
    args: &Args,
    mode: &'static str,
    device: DeviceReport,
    diagnostics: OutputDiagnostics,
    stalls: u64,
    regressions: u64,
    reopen_attempts: u64,
    reopen_successes: u64,
    seeks: u64,
    rate_changes: u64,
    click_changes: u64,
    samples: Vec<Sample>,
    failures: Vec<String>,
) -> Report {
    let rss: Vec<u64> = samples
        .iter()
        .filter_map(|sample| sample.rss_bytes)
        .collect();
    let private: Vec<u64> = samples
        .iter()
        .filter_map(|sample| sample.private_bytes)
        .collect();
    let rss_start = rss.first().copied();
    let rss_end = rss.last().copied();
    Report {
        schema: "drillforge.audio-endurance.v1",
        started_unix_ms,
        requested_duration_ms: args.duration.as_millis() as u64,
        actual_duration_ms: if mode == "mock" {
            0
        } else {
            args.duration.as_millis() as u64
        },
        mode,
        device,
        passed: failures.is_empty(),
        callbacks: diagnostics.callbacks,
        rendered_frames: diagnostics.rendered_frames,
        underruns: diagnostics.underruns,
        callback_stalls: stalls,
        clock_regressions: regressions,
        device_errors: diagnostics.device_errors,
        reopen_attempts,
        reopen_successes,
        seeks,
        rate_changes,
        click_program_changes: click_changes,
        rss_start_bytes: rss_start,
        rss_peak_bytes: rss.iter().copied().max(),
        rss_end_bytes: rss_end,
        rss_growth_bytes: rss_start
            .zip(rss_end)
            .map(|(start, end)| end as i64 - start as i64),
        private_start_bytes: private.first().copied(),
        private_peak_bytes: private.iter().copied().max(),
        private_end_bytes: private.last().copied(),
        samples_truncated: samples.len() == MAX_REPORT_SAMPLES,
        samples,
        failures,
    }
}

#[cfg(windows)]
fn process_memory() -> (Option<u64>, Option<u64>) {
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        peak_paged_pool: usize,
        paged_pool: usize,
        peak_nonpaged_pool: usize,
        nonpaged_pool: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private_usage: usize,
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, size: u32) -> i32;
    }
    let mut counters = Counters {
        cb: size_of::<Counters>() as u32,
        page_faults: 0,
        peak_working_set: 0,
        working_set: 0,
        peak_paged_pool: 0,
        paged_pool: 0,
        peak_nonpaged_pool: 0,
        nonpaged_pool: 0,
        pagefile: 0,
        peak_pagefile: 0,
        private_usage: 0,
    };
    // SAFETY: the pseudo-handle is process-local and the pointer/size match Counters.
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            size_of::<Counters>() as u32,
        )
    };
    if ok == 0 {
        (None, None)
    } else {
        (
            Some(counters.working_set as u64),
            Some(counters.private_usage as u64),
        )
    }
}

#[cfg(not(windows))]
fn process_memory() -> (Option<u64>, Option<u64>) {
    (None, None)
}
