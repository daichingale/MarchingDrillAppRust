#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 2_097_152 { return; }
    let path = std::env::temp_dir().join(format!("drillforge-fuzz-{}.drillproj", std::process::id()));
    if std::fs::write(&path, data).is_ok() {
        let _ = drill_project::container::load(&path);
        let _ = std::fs::remove_file(path);
    }
});
