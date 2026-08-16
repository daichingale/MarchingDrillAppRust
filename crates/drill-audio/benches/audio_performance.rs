use drill_audio::{
    AudioAsset, ClickMixer, ClickSchedule, ClickSettings, ClickVoices, MixerState, Peak,
    PeakPyramid, PitchQuality, PlaybackRate, RateProcessor, render_block,
};
use drill_core::tempo::TempoMap;
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

struct CountingAllocator;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn main() {
    let frames = 10 * 60 * 48_000;
    let samples = vec![0_i16; frames * 2];
    let asset = AudioAsset::from_interleaved(samples, 48_000, 2, 1.0).unwrap();
    let started = Instant::now();
    let peaks = PeakPyramid::build(black_box(&asset));
    let build_elapsed = started.elapsed();
    let mut output = Vec::<Peak>::with_capacity(3_840);
    let started = Instant::now();
    for _ in 0..1_000 {
        peaks.range(&asset, 0, asset.frames(), 1_920, &mut output);
        black_box(&output);
    }
    let range_elapsed = started.elapsed() / 1_000;
    println!("10-minute stereo peak build: {build_elapsed:?}");
    println!("1920px peak range: {range_elapsed:?}");

    let mut mixer = MixerState::new();
    let mut block = [0.0_f32; 1_024];
    let started = Instant::now();
    for _ in 0..10_000 {
        render_block(&asset, 48_000, 2, 1.0, false, &mut mixer, &mut block);
        black_box(&block);
    }
    println!("512-frame stereo mix: {:?}", started.elapsed() / 10_000);

    let settings = ClickSettings {
        enabled: true,
        ..ClickSettings::default()
    };
    let schedule =
        ClickSchedule::build(&TempoMap::constant(120.0), 0.0, 2_048.0, &settings, 48_000);
    let voices = ClickVoices::render(&settings, 48_000);
    let mut click_mixer = ClickMixer::default();
    let started = Instant::now();
    for block_index in 0..10_000_i64 {
        block.fill(0.0);
        click_mixer.add_block(&schedule, &voices, block_index * 512, 2, &mut block);
        black_box(&block);
    }
    println!("512-frame click mix: {:?}", started.elapsed() / 10_000);

    let mut processor = RateProcessor::new(2, PitchQuality::Standard).unwrap();
    let rate = PlaybackRate::new(0.75).unwrap();
    let started = Instant::now();
    for _ in 0..1_000 {
        processor.render(&asset, rate, None, 1.0, false, &mut block);
        black_box(&block);
    }
    println!(
        "512-frame standard pitch preserve: {:?}",
        started.elapsed() / 1_000
    );

    // Two hours at 48 kHz, advanced through the exact device-independent
    // renderer used by the mixer thread. A one-second source is repeatedly
    // sought to model loop playback without allocating a two-hour PCM asset.
    let soak_asset =
        AudioAsset::from_interleaved(vec![0_i16; 48_000 * 2], 48_000, 2, 1.0).expect("soak asset");
    let mut soak_state = MixerState::new();
    let mut soak_block = [0.0_f32; 1_024];
    render_block(
        &soak_asset,
        48_000,
        2,
        1.0,
        false,
        &mut soak_state,
        &mut soak_block,
    );
    let resident_before = soak_asset.resident_bytes();
    let allocations_before = ALLOCATIONS.load(Ordering::Relaxed);
    let blocks = (2_u64 * 60 * 60 * 48_000).div_ceil(512);
    let started = Instant::now();
    for block_index in 0..blocks {
        if block_index % 93 == 0 {
            soak_state.seek_source_frame(0, 48_000, 48_000);
        }
        render_block(
            &soak_asset,
            48_000,
            2,
            1.0,
            false,
            &mut soak_state,
            &mut soak_block,
        );
        black_box(&soak_block);
    }
    let soak_elapsed = started.elapsed();
    let soak_allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations_before;
    println!(
        "2-hour PCM hot-path simulation ({blocks} blocks): {soak_elapsed:?}, allocations={soak_allocations}"
    );
    assert_eq!(
        soak_asset.resident_bytes(),
        resident_before,
        "resident PCM storage grew"
    );
    assert_eq!(
        soak_allocations, 0,
        "audio hot path allocated during two-hour soak"
    );
}
