#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() <= 1_048_576 {
        let mut options = drill_interop::musical::MusicalImportOptions::default();
        options.max_bytes = 1_048_576;
        let _ = drill_interop::musical::import_midi(data, &options);
    }
});
