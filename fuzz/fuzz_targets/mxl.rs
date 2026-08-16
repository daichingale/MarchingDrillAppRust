#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() <= 2_097_152 {
        let mut options = drill_interop::musical::MusicalImportOptions::default();
        options.max_bytes = 2_097_152;
        let _ = drill_interop::musical::import_musicxml(data, &options);
    }
});
