use drill_audio::{AudioAsset, AudioOutput, OutputError, probe_default_output};
use std::{process::ExitCode, sync::Arc, thread, time::Duration};

fn main() -> ExitCode {
    let device = match probe_default_output() {
        Ok(device) => device,
        Err(OutputError::NoDefaultDevice) => {
            eprintln!("SKIP: no default audio output device is available");
            return ExitCode::from(77);
        }
        Err(error) => {
            eprintln!("FAIL: audio output probe failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "DEVICE: {} / {} Hz / {} ch / {}",
        device.name, device.sample_rate, device.channels, device.sample_format
    );
    let frames = usize::try_from(device.sample_rate).unwrap_or(48_000);
    let asset = AudioAsset::from_interleaved(vec![0_i16; frames], 48_000, 1, 1.0)
        .expect("valid diagnostic silence");
    let output = match AudioOutput::open_default(Arc::new(asset)) {
        Ok(output) => output,
        Err(error) => {
            eprintln!("FAIL: device was found but its stream could not start: {error}");
            return ExitCode::FAILURE;
        }
    };
    output.play();
    thread::sleep(Duration::from_millis(350));
    let sample = output.clock().sample();
    output.pause();
    if sample.playing && sample.position > 0 {
        println!(
            "PASS: callback advanced to output frame {}",
            sample.position
        );
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "FAIL: callback did not advance (playing={}, frame={})",
            sample.playing, sample.position
        );
        ExitCode::FAILURE
    }
}
