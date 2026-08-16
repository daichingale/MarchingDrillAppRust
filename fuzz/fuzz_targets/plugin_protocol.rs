#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() <= 262_144 {
        let mut limits = drill_plugin::PluginLimits::DEFAULT;
        limits.request_bytes = 262_144;
        limits.output_bytes = 262_144;
        let _ = drill_plugin::decode_command(data, limits);
    }
});
