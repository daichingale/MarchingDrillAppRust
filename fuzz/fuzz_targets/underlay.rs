#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() <= 2_097_152 {
        let limits = drill_interop::underlay::UnderlayLimits { max_bytes: 2_097_152, max_width: 4096, max_height: 4096, max_pixels: 4_194_304 };
        let _ = drill_interop::underlay::decode_underlay(data, limits);
    }
});
