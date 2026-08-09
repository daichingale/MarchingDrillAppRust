#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = drill_interop::ImportLimits { max_bytes: 2_097_152, max_rows: 4096, max_columns: 64, ..Default::default() };
    if data.len() <= limits.max_bytes { let _ = drill_interop::xlsx::inspect_xlsx(data, &limits); }
});
