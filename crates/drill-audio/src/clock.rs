use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};

#[derive(Debug)]
pub(crate) struct ClockShared {
    sequence: AtomicU32,
    position: AtomicI64,
    source_position: AtomicU64,
    sample_rate: AtomicU32,
    playing: AtomicBool,
}

impl ClockShared {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            sequence: AtomicU32::new(0),
            position: AtomicI64::new(0),
            source_position: AtomicU64::new(0),
            sample_rate: AtomicU32::new(sample_rate),
            playing: AtomicBool::new(false),
        }
    }

    pub(crate) fn write(
        &self,
        position: i64,
        source_position: u64,
        sample_rate: u32,
        playing: bool,
    ) {
        // AcqRel on the odd transition prevents the payload stores from being
        // reordered before readers can observe the write-in-progress marker.
        self.sequence.fetch_add(1, Ordering::AcqRel);
        self.position.store(position, Ordering::Relaxed);
        self.source_position
            .store(source_position, Ordering::Relaxed);
        self.sample_rate.store(sample_rate, Ordering::Relaxed);
        self.playing.store(playing, Ordering::Relaxed);
        self.sequence.fetch_add(1, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockSample {
    /// Interleaved-frame position at the output device sample rate.
    pub position: i64,
    /// Q32 source-frame position, allowing exact source/output correspondence.
    pub source_position_q32: u64,
    pub sample_rate: u32,
    pub playing: bool,
}

#[derive(Clone, Debug)]
pub struct PlaybackClock {
    pub(crate) shared: Arc<ClockShared>,
}

impl PlaybackClock {
    pub(crate) fn new(shared: Arc<ClockShared>) -> Self {
        Self { shared }
    }

    /// Reads a coherent clock snapshot without locks.
    #[must_use]
    pub fn sample(&self) -> ClockSample {
        loop {
            let before = self.shared.sequence.load(Ordering::Acquire);
            if before & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let sample = ClockSample {
                position: self.shared.position.load(Ordering::Relaxed),
                source_position_q32: self.shared.source_position.load(Ordering::Relaxed),
                sample_rate: self.shared.sample_rate.load(Ordering::Relaxed),
                playing: self.shared.playing.load(Ordering::Relaxed),
            };
            let after = self.shared.sequence.load(Ordering::Acquire);
            if before == after {
                return sample;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_never_returns_torn_samples() {
        let shared = Arc::new(ClockShared::new(48_000));
        let clock = PlaybackClock::new(Arc::clone(&shared));
        let writer = std::thread::spawn(move || {
            for position in 1..50_000 {
                shared.write(
                    position,
                    (position as u64) << 32,
                    position as u32,
                    position & 1 == 0,
                );
            }
        });
        while !writer.is_finished() {
            let sample = clock.sample();
            if sample.position > 0 {
                assert_eq!(sample.position as u32, sample.sample_rate);
                assert_eq!(sample.source_position_q32 >> 32, sample.position as u64);
            }
        }
        writer.join().unwrap();
    }
}
