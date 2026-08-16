use drill_core::{Document, Locale, SetId};
use std::time::{Duration, Instant};

fn main() {
    const MAX_TIME: Duration = Duration::from_secs(2);
    const MAX_BYTES: usize = 64 * 1024 * 1024;
    let mut document = Document::demo(25, 40);
    let template = document.sets[0].clone();
    for raw in 3..=64_u32 {
        let mut set = template.clone();
        set.id = SetId::new(raw).unwrap();
        set.name = format!("Set {raw}");
        document.sets.push(set);
    }
    document.validate().unwrap();
    let started = Instant::now();
    let html = drill_mobile_viewer::build_practice_viewer(&document, &[], Locale::Ja).unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed <= MAX_TIME,
        "mobile viewer generation {elapsed:?} > {MAX_TIME:?}"
    );
    assert!(
        html.len() <= MAX_BYTES,
        "mobile viewer {} bytes > {MAX_BYTES}",
        html.len()
    );
    let digest = blake3::hash(html.as_bytes());
    println!(
        "mobile_viewer_1000x64: elapsed={elapsed:?}; bytes={}; digest={digest}",
        html.len()
    );
}
