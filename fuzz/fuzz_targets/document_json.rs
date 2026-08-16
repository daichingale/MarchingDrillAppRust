#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 1_048_576 { return; }
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(document) = drill_core::Document::from_json(text) {
            let encoded = document.to_json().expect("accepted documents serialize");
            drill_core::Document::from_json(&encoded).expect("serialized documents reload");
        }
    }
});
