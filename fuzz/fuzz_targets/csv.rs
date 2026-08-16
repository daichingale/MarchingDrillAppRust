#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = drill_interop::ImportLimits { max_bytes: 1_048_576, max_rows: 4096, max_columns: 64, max_field_bytes: 4096, ..Default::default() };
    if data.len() <= limits.max_bytes { let _ = drill_interop::sniff(data, &limits); }
});
