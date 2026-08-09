//! Opt-in, real-process video export conformance gate.
//!
//! Exit code 77 means the host does not have an explicitly configured or
//! PATH-resolvable FFmpeg/ffprobe pair.  It is intentionally not a passing
//! test result; wrappers decide whether an unavailable host is allowed.

use drill_core::Document;
use drill_core::video::{EncoderBackend, VideoExportConfig};
use drill_export::{
    CountRange, ReplacePolicy, VideoExportRequest, spawn_video_export,
    spawn_video_export_with_policy,
};
use drill_jobs::JobMsg;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const UNAVAILABLE: u8 = 77;
const FPS: u32 = 12;
const EXPECTED_SECONDS: f64 = 1.0;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Unavailable(reason)) => {
            eprintln!("SKIP-UNAVAILABLE: {reason}");
            ExitCode::from(UNAVAILABLE)
        }
        Err(Failure::Failed(reason)) => {
            eprintln!("VIDEO-E2E-FAILED: {reason}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
enum Failure {
    Unavailable(String),
    Failed(String),
}

impl From<std::io::Error> for Failure {
    fn from(value: std::io::Error) -> Self {
        Self::Failed(value.to_string())
    }
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run() -> Result<(), Failure> {
    let ffmpeg = resolve_tool("DRILLFORGE_FFMPEG", "ffmpeg")?;
    let ffprobe = resolve_tool("DRILLFORGE_FFPROBE", "ffprobe")?;
    verify_tool(&ffmpeg, "ffmpeg")?;
    verify_tool(&ffprobe, "ffprobe")?;
    verify_software_encoder(&ffmpeg)?;

    let scratch = Scratch(unique_scratch());
    fs::create_dir(&scratch.0)?;
    let audio = scratch.0.join("tone.wav");
    write_test_wav(&audio)?;
    let output = scratch.0.join("show.mp4");
    let export_request = request(&ffmpeg, &ffprobe, &audio, &output);

    let first = wait(
        spawn_video_export(export_request.clone()),
        Duration::from_secs(60),
    )?;
    assert_report(&first, &output, false)?;
    let first_hashes = frame_hashes(&ffmpeg, &output)?;
    if first_hashes
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .count()
        != FPS as usize
    {
        return Err(Failure::Failed(format!(
            "decoded frame count was not {FPS}:\n{first_hashes}"
        )));
    }
    probe_media(&ffprobe, &output)?;
    let original = fs::read(&output)?;

    let replaced = wait(
        spawn_video_export_with_policy(export_request.clone(), ReplacePolicy::BackupAndReplace),
        Duration::from_secs(60),
    )?;
    assert_report(&replaced, &output, true)?;
    let backup = replaced
        .backup
        .as_ref()
        .ok_or_else(|| Failure::Failed("approved replacement did not retain a backup".into()))?;
    if fs::read(backup)? != original {
        return Err(Failure::Failed(
            "replacement backup differs from old output".into(),
        ));
    }
    let replacement_hashes = frame_hashes(&ffmpeg, &output)?;
    if replacement_hashes != first_hashes {
        return Err(Failure::Failed(
            "repeated export changed decoded frame hashes".into(),
        ));
    }

    let cancelled_output = scratch.0.join("cancelled.mp4");
    let mut cancelled = spawn_video_export(request(&ffmpeg, &ffprobe, &audio, &cancelled_output));
    cancelled.cancel();
    match wait_message(&mut cancelled, Duration::from_secs(60))? {
        JobMsg::Cancelled => {}
        other => return Err(Failure::Failed(format!("cancel returned {other:?}"))),
    }
    if cancelled_output.exists() || partials_for(&scratch.0, "cancelled").next().is_some() {
        return Err(Failure::Failed(
            "cancel left a final or partial output behind".into(),
        ));
    }

    println!(
        "VIDEO-E2E-PASS ffmpeg={} ffprobe={} frames={FPS} audio=true duration={EXPECTED_SECONDS:.3}s hashes=stable replace=atomic cancel=clean",
        ffmpeg.display(),
        ffprobe.display()
    );
    Ok(())
}

fn request(ffmpeg: &Path, ffprobe: &Path, audio: &Path, output: &Path) -> VideoExportRequest {
    VideoExportRequest {
        document: Document::demo(2, 3),
        config: VideoExportConfig {
            width: 160,
            height: 90,
            fps: FPS,
            backend: EncoderBackend::Software,
            audio_enabled: true,
            ..VideoExportConfig::default()
        },
        range: CountRange {
            start: 0.0,
            end: 2.0,
        },
        output: output.to_path_buf(),
        ffmpeg: ffmpeg.to_path_buf(),
        ffprobe: ffprobe.to_path_buf(),
        audio: Some(audio.to_path_buf()),
        underlay: None,
        use_3d_camera: false,
    }
}

fn wait(
    mut job: drill_jobs::Job<drill_export::ExportReport>,
    timeout: Duration,
) -> Result<drill_export::ExportReport, Failure> {
    match wait_message(&mut job, timeout)? {
        JobMsg::Done(report) => Ok(report),
        JobMsg::Failed(reason) => Err(Failure::Failed(reason.to_string())),
        JobMsg::Cancelled => Err(Failure::Failed("export was unexpectedly cancelled".into())),
    }
}

fn wait_message<T: Send + 'static>(
    job: &mut drill_jobs::Job<T>,
    timeout: Duration,
) -> Result<JobMsg<T>, Failure> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(message) = job.poll() {
            return Ok(message);
        }
        if Instant::now() >= deadline {
            job.cancel();
            return Err(Failure::Failed("export timed out".into()));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn assert_report(
    report: &drill_export::ExportReport,
    output: &Path,
    replacement: bool,
) -> Result<(), Failure> {
    if report.frames != u64::from(FPS) || report.output != output || !output.is_file() {
        return Err(Failure::Failed(format!(
            "invalid export report: {report:?}"
        )));
    }
    if report.used_software_fallback {
        return Err(Failure::Failed(
            "software export unexpectedly reported fallback".into(),
        ));
    }
    if replacement != report.backup.is_some() {
        return Err(Failure::Failed(
            "replacement backup state is incorrect".into(),
        ));
    }
    Ok(())
}

fn probe_media(ffprobe: &Path, output: &Path) -> Result<(), Failure> {
    let result = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,avg_frame_rate:format=duration",
            "-of",
            "default=nw=1",
        ])
        .arg(output)
        .output()?;
    if !result.status.success() {
        return Err(Failure::Failed(format!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    let text = String::from_utf8_lossy(&result.stdout);
    let video = text.lines().any(|line| line == "codec_type=video");
    let audio = text.lines().any(|line| line == "codec_type=audio");
    let fps = text
        .lines()
        .find_map(|line| line.strip_prefix("avg_frame_rate="))
        .and_then(parse_ratio);
    let duration = text
        .lines()
        .find_map(|line| line.strip_prefix("duration="))
        .and_then(|value| value.parse::<f64>().ok());
    if !video || !audio || fps.is_none_or(|value| (value - f64::from(FPS)).abs() > 0.01) {
        return Err(Failure::Failed(format!("unexpected streams/FPS:\n{text}")));
    }
    if duration.is_none_or(|value| (value - EXPECTED_SECONDS).abs() > 0.08) {
        return Err(Failure::Failed(format!("unexpected duration:\n{text}")));
    }
    Ok(())
}

fn frame_hashes(ffmpeg: &Path, output: &Path) -> Result<String, Failure> {
    let result = Command::new(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(output)
        .args(["-map", "0:v:0", "-f", "framemd5", "-"])
        .output()?;
    if !result.status.success() {
        return Err(Failure::Failed(format!(
            "framemd5 failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

fn parse_ratio(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    (denominator != 0.0).then_some(numerator / denominator)
}

fn resolve_tool(variable: &str, name: &str) -> Result<PathBuf, Failure> {
    if let Some(configured) = std::env::var_os(variable) {
        let candidate = PathBuf::from(configured);
        if !candidate.is_absolute() {
            return Err(Failure::Failed(format!(
                "{variable} must be an absolute path"
            )));
        }
        return canonical_tool(&candidate, variable);
    }
    let Some(path) = std::env::var_os("PATH") else {
        return Err(Failure::Unavailable(format!(
            "{name} not configured and PATH is empty"
        )));
    };
    for directory in std::env::split_paths(&path) {
        for candidate in [directory.join(format!("{name}.exe")), directory.join(name)] {
            if candidate.is_file() {
                return canonical_tool(&candidate, name);
            }
        }
    }
    Err(Failure::Unavailable(format!(
        "{name} not found; set {variable} to its absolute path"
    )))
}

fn canonical_tool(path: &Path, label: &str) -> Result<PathBuf, Failure> {
    if !path.is_file() {
        return Err(Failure::Unavailable(format!(
            "{label} does not name a file: {}",
            path.display()
        )));
    }
    fs::canonicalize(path)
        .map_err(|error| Failure::Failed(format!("cannot resolve {label}: {error}")))
}

fn verify_tool(path: &Path, label: &str) -> Result<(), Failure> {
    let output = Command::new(path).arg("-version").output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Failure::Unavailable(format!("{label} -version failed")))
    }
}

fn verify_software_encoder(ffmpeg: &Path) -> Result<(), Failure> {
    let output = Command::new(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    if output.status.success() && text.contains("libx264") {
        Ok(())
    } else {
        Err(Failure::Unavailable(
            "FFmpeg is present but has no libx264 encoder".into(),
        ))
    }
}

fn unique_scratch() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "drillforge-video-e2e-{}-{nonce}",
        std::process::id()
    ))
}

fn partials_for<'a>(directory: &'a Path, stem: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(move |path| {
            path.file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.starts_with(&format!(".{stem}.partial-")))
        })
}

fn write_test_wav(path: &Path) -> Result<(), Failure> {
    const SAMPLE_RATE: u32 = 48_000;
    const SAMPLES: u32 = SAMPLE_RATE;
    const CHANNELS: u16 = 1;
    const BITS: u16 = 16;
    let data_bytes = SAMPLES * u32::from(CHANNELS) * u32::from(BITS / 8);
    let mut file = fs::File::create(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_bytes).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&CHANNELS.to_le_bytes())?;
    file.write_all(&SAMPLE_RATE.to_le_bytes())?;
    file.write_all(&(SAMPLE_RATE * u32::from(CHANNELS) * u32::from(BITS / 8)).to_le_bytes())?;
    file.write_all(&(CHANNELS * BITS / 8).to_le_bytes())?;
    file.write_all(&BITS.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_bytes.to_le_bytes())?;
    for sample in 0..SAMPLES {
        let phase = 2.0 * std::f32::consts::PI * 440.0 * sample as f32 / SAMPLE_RATE as f32;
        let value = (phase.sin() * 4_000.0) as i16;
        file.write_all(&value.to_le_bytes())?;
    }
    file.sync_all()?;
    Ok(())
}
