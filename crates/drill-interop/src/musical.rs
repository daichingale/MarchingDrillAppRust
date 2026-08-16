//! Bounded import of the musical timeline subset of MusicXML and Standard MIDI.
//! Notes are intentionally ignored: importing a score must never invent performers.

use drill_core::tempo::{TempoChange, TempoMap};
use std::collections::BTreeSet;
use std::io::{Cursor, Read};

const MAX_MIDI_TRACKS: usize = 256;
const MAX_MIDI_EVENTS: usize = 2_000_000;
const MAX_XML_NODES: usize = 500_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountUnit {
    Whole,
    Half,
    Quarter,
    Eighth,
    DottedQuarter,
}
impl CountUnit {
    pub const fn quarters(self) -> f64 {
        match self {
            Self::Whole => 4.0,
            Self::Half => 2.0,
            Self::Quarter => 1.0,
            Self::Eighth => 0.5,
            Self::DottedQuarter => 1.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MusicalImportOptions {
    pub count_unit: CountUnit,
    pub compound_meter_is_dotted: bool,
    pub start_measure: u32,
    pub max_measures: u32,
    pub max_marks: usize,
    pub max_bytes: u64,
}
impl Default for MusicalImportOptions {
    fn default() -> Self {
        Self {
            count_unit: CountUnit::Quarter,
            compound_meter_is_dotted: true,
            start_measure: 1,
            max_measures: 4_000,
            max_marks: 4_000,
            max_bytes: 32 << 20,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterChange {
    pub measure: u32,
    pub count: f32,
    pub numerator: u8,
    pub denominator: u8,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasureStart {
    pub measure: u32,
    pub count: f32,
    pub implicit: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Rehearsal,
    Marker,
    Text,
    Segno,
    Coda,
}
#[derive(Clone, Debug, PartialEq)]
pub struct MusicalMark {
    pub measure: u32,
    pub count: f32,
    pub text: String,
    pub kind: MarkKind,
}
#[derive(Clone, Debug, Default)]
pub struct MusicalTimeline {
    pub tempo: TempoMap,
    pub meters: Vec<MeterChange>,
    pub measures: Vec<MeasureStart>,
    pub marks: Vec<MusicalMark>,
    pub total_counts: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MusicalWarning {
    ZeroTempoIgnored,
    InvalidMeterIgnored,
    TextLossy,
    MeasuresTruncated,
    MarksTruncated,
    MidMeasureTempoRounded,
}
#[derive(Clone, Debug)]
pub struct MusicalImport {
    pub timeline: MusicalTimeline,
    pub warnings: Vec<MusicalWarning>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MusicalError {
    TooLarge { limit: u64 },
    Invalid(&'static str),
    UnsupportedSmpte,
    Limit(&'static str),
}
impl std::fmt::Display for MusicalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { limit } => {
                write!(f, "music file exceeds safety limit ({limit} bytes)")
            }
            Self::Invalid(s) => write!(f, "invalid musical file: {s}"),
            Self::UnsupportedSmpte => {
                f.write_str("SMPTE MIDI timing has no beat tempo map; export metrical MIDI")
            }
            Self::Limit(s) => write!(f, "musical file exceeds safety limit: {s}"),
        }
    }
}
impl std::error::Error for MusicalError {}

fn bounded(bytes: &[u8], o: &MusicalImportOptions) -> Result<(), MusicalError> {
    if bytes.len() as u64 > o.max_bytes {
        Err(MusicalError::TooLarge { limit: o.max_bytes })
    } else {
        Ok(())
    }
}
fn clean_text(bytes: &[u8], warnings: &mut Vec<MusicalWarning>) -> String {
    let bytes = &bytes[..bytes.len().min(256)];
    match std::str::from_utf8(bytes) {
        Ok(s) => s.trim().to_owned(),
        Err(_) => {
            push_warning(warnings, MusicalWarning::TextLossy);
            String::from_utf8_lossy(bytes).trim().to_owned()
        }
    }
}

fn push_warning(warnings: &mut Vec<MusicalWarning>, warning: MusicalWarning) {
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

/// Parse SMF format 0/1. The parser skips channel events without allocating them.
pub fn import_midi(
    bytes: &[u8],
    opts: &MusicalImportOptions,
) -> Result<MusicalImport, MusicalError> {
    bounded(bytes, opts)?;
    let mut p = 0;
    if take(bytes, &mut p, 4)? != b"MThd" {
        return Err(MusicalError::Invalid("missing MThd"));
    }
    let hlen = be32(bytes, &mut p)? as usize;
    let header_end = p
        .checked_add(hlen)
        .filter(|&x| x <= bytes.len())
        .ok_or(MusicalError::Invalid("header length"))?;
    if hlen < 6 {
        return Err(MusicalError::Invalid("header length"));
    }
    let format = be16(bytes, &mut p)?;
    let tracks = be16(bytes, &mut p)? as usize;
    let division = be16(bytes, &mut p)?;
    p = header_end;
    if format > 1 {
        return Err(MusicalError::Invalid("SMF format 2 is unsupported"));
    }
    if tracks == 0 || tracks > MAX_MIDI_TRACKS {
        return Err(MusicalError::Limit("track count"));
    }
    if division & 0x8000 != 0 {
        return Err(MusicalError::UnsupportedSmpte);
    }
    if division == 0 {
        return Err(MusicalError::Invalid("zero MIDI division"));
    }
    let unit_ticks = f64::from(division) * opts.count_unit.quarters();
    let mut tempo = Vec::new();
    let mut meters = Vec::new();
    let mut marks = Vec::new();
    let mut warnings = Vec::new();
    let mut max_tick = 0u64;
    let mut events = 0usize;
    for _ in 0..tracks {
        if take(bytes, &mut p, 4)? != b"MTrk" {
            return Err(MusicalError::Invalid("missing MTrk"));
        }
        let len = be32(bytes, &mut p)? as usize;
        let end = p
            .checked_add(len)
            .filter(|&x| x <= bytes.len())
            .ok_or(MusicalError::Invalid("track length"))?;
        let mut tick = 0u64;
        let mut running = None;
        while p < end {
            events += 1;
            if events > MAX_MIDI_EVENTS {
                return Err(MusicalError::Limit("event count"));
            }
            tick = tick.saturating_add(vlq(bytes, &mut p, end)?);
            max_tick = max_tick.max(tick);
            let first = *bytes.get(p).ok_or(MusicalError::Invalid("event"))?;
            let status = if first & 0x80 != 0 {
                p += 1;
                running = Some(first);
                first
            } else {
                running.ok_or(MusicalError::Invalid("running status"))?
            };
            if status == 0xff {
                running = None;
                let kind = *bytes.get(p).ok_or(MusicalError::Invalid("meta type"))?;
                p += 1;
                let n = vlq(bytes, &mut p, end)? as usize;
                let data = take_to(bytes, &mut p, n, end)?;
                let count = (tick as f64 / unit_ticks) as f32;
                match kind {
                    0x51 if data.len() == 3 => {
                        let us = (u32::from(data[0]) << 16)
                            | (u32::from(data[1]) << 8)
                            | u32::from(data[2]);
                        if us == 0 {
                            push_warning(&mut warnings, MusicalWarning::ZeroTempoIgnored)
                        } else {
                            if tempo.len() < opts.max_measures as usize {
                                tempo.push(TempoChange {
                                    count,
                                    bpm: (60_000_000.0 / f64::from(us) / opts.count_unit.quarters())
                                        as f32,
                                });
                            } else {
                                push_warning(&mut warnings, MusicalWarning::MeasuresTruncated);
                            }
                        }
                    }
                    0x58 if data.len() >= 2 => {
                        let den = 1u16.checked_shl(u32::from(data[1])).filter(|&d| d <= 128);
                        if let Some(d) = den {
                            if meters.len() < opts.max_measures as usize {
                                meters.push(MeterChange {
                                    measure: 0,
                                    count,
                                    numerator: data[0],
                                    denominator: d as u8,
                                })
                            } else {
                                push_warning(&mut warnings, MusicalWarning::MeasuresTruncated);
                            }
                        } else {
                            push_warning(&mut warnings, MusicalWarning::InvalidMeterIgnored)
                        }
                    }
                    0x06 | 0x07 | 0x01 if marks.len() < opts.max_marks => {
                        let kind = if kind == 0x06 {
                            MarkKind::Marker
                        } else {
                            MarkKind::Text
                        };
                        let text = clean_text(data, &mut warnings);
                        if !text.is_empty() {
                            marks.push(MusicalMark {
                                measure: 0,
                                count,
                                text,
                                kind,
                            });
                        }
                    }
                    0x06 | 0x07 | 0x01 => {
                        push_warning(&mut warnings, MusicalWarning::MarksTruncated)
                    }
                    _ => {}
                }
            } else if status == 0xf0 || status == 0xf7 {
                running = None;
                let n = vlq(bytes, &mut p, end)? as usize;
                take_to(bytes, &mut p, n, end)?;
            } else {
                let n = match status & 0xf0 {
                    0xc0 | 0xd0 => 1,
                    0x80 | 0x90 | 0xa0 | 0xb0 | 0xe0 => 2,
                    _ => return Err(MusicalError::Invalid("MIDI status")),
                };
                take_to(bytes, &mut p, n, end)?;
            }
        }
        p = end;
    }
    if tempo.is_empty() {
        tempo.push(TempoChange {
            count: 0.0,
            bpm: 120.0,
        });
    }
    tempo.sort_by(|a, b| a.count.total_cmp(&b.count));
    meters.sort_by(|a, b| a.count.total_cmp(&b.count));
    marks.sort_by(|a, b| a.count.total_cmp(&b.count));
    Ok(MusicalImport {
        timeline: MusicalTimeline {
            tempo: TempoMap::from_changes(tempo),
            meters,
            measures: vec![],
            marks,
            total_counts: (max_tick as f64 / unit_ticks).ceil().min(u32::MAX as f64) as u32,
        },
        warnings,
    })
}

/// Parse MusicXML or its compressed `.mxl` container.
pub fn import_musicxml(
    bytes: &[u8],
    opts: &MusicalImportOptions,
) -> Result<MusicalImport, MusicalError> {
    bounded(bytes, opts)?;
    let extracted;
    let xml = if bytes.starts_with(b"PK\x03\x04") {
        extracted = extract_mxl(bytes, opts)?;
        extracted.as_slice()
    } else {
        bytes
    };
    let s =
        std::str::from_utf8(xml).map_err(|_| MusicalError::Invalid("MusicXML must be UTF-8"))?;
    if s.contains("<!DOCTYPE") || s.contains("<!ENTITY") {
        return Err(MusicalError::Invalid("DTD/entities are forbidden"));
    }
    let nodes = s.as_bytes().iter().filter(|&&b| b == b'<').count();
    if nodes > MAX_XML_NODES {
        return Err(MusicalError::Limit("XML nodes"));
    }
    if !s.contains("<score-partwise") && !s.contains("<score-timewise") {
        return Err(MusicalError::Invalid("score root"));
    }
    // Partwise scores repeat each measure for every instrument. Measure geometry
    // comes from the first part only; otherwise total counts multiply by part count.
    let scan = if s.contains("<score-partwise") {
        let part = find_tag(s, 0, "part").ok_or(MusicalError::Invalid("no score part"))?;
        let start = s[part..]
            .find('>')
            .map(|x| part + x + 1)
            .ok_or(MusicalError::Invalid("part tag"))?;
        let end = s[start..]
            .find("</part>")
            .map(|x| start + x)
            .ok_or(MusicalError::Invalid("part close"))?;
        &s[start..end]
    } else {
        s
    };
    let mut out = MusicalTimeline::default();
    let mut warnings = Vec::new();
    let mut cursor = 0usize;
    let mut count = 0f32;
    let mut meter = (4u8, 4u8);
    let mut measure_no = 0u32;
    while let Some(ms) = find_tag(scan, cursor, "measure") {
        let open_end = scan[ms..]
            .find('>')
            .map(|x| ms + x + 1)
            .ok_or(MusicalError::Invalid("measure tag"))?;
        let close = scan[open_end..]
            .find("</measure>")
            .map(|x| open_end + x)
            .ok_or(MusicalError::Invalid("measure close"))?;
        let open = &scan[ms..open_end];
        let body = &scan[open_end..close];
        measure_no = attr(open, "number")
            .and_then(|x| x.parse().ok())
            .unwrap_or(measure_no + 1);
        cursor = close + 10;
        if let (Some(n), Some(d)) = (
            tag_text(body, "beats").and_then(|x| x.parse::<u8>().ok()),
            tag_text(body, "beat-type").and_then(|x| x.parse::<u8>().ok()),
        ) {
            if n > 0 && d.is_power_of_two() {
                meter = (n, d);
            } else {
                push_warning(&mut warnings, MusicalWarning::InvalidMeterIgnored)
            }
        }
        if measure_no < opts.start_measure {
            continue;
        }
        if out.measures.len() >= opts.max_measures as usize {
            push_warning(&mut warnings, MusicalWarning::MeasuresTruncated);
            break;
        }
        let implicit = attr(open, "implicit") == Some("yes");
        out.measures.push(MeasureStart {
            measure: measure_no,
            count,
            implicit,
        });
        if out
            .meters
            .last()
            .is_none_or(|change| change.numerator != meter.0 || change.denominator != meter.1)
        {
            out.meters.push(MeterChange {
                measure: measure_no,
                count,
                numerator: meter.0,
                denominator: meter.1,
            });
        }
        let unit = if opts.compound_meter_is_dotted && meter.1 == 8 && meter.0.is_multiple_of(3) {
            CountUnit::DottedQuarter
        } else {
            opts.count_unit
        };
        let mut from = 0;
        while let Some(rel) = body[from..].find("<sound") {
            let st = from + rel;
            let en = body[st..]
                .find('>')
                .map(|x| st + x + 1)
                .ok_or(MusicalError::Invalid("sound tag"))?;
            if let Some(v) = attr(&body[st..en], "tempo")
                .and_then(|x| x.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
            {
                out.tempo.set(count, (v / unit.quarters()) as f32);
            }
            from = en;
        }
        let mut from = 0;
        while let Some(rel) = body[from..].find("<rehearsal") {
            if out.marks.len() >= opts.max_marks {
                push_warning(&mut warnings, MusicalWarning::MarksTruncated);
                break;
            }
            let st = from + rel;
            let gt = body[st..]
                .find('>')
                .map(|x| st + x + 1)
                .ok_or(MusicalError::Invalid("rehearsal"))?;
            let en = body[gt..]
                .find("</rehearsal>")
                .map(|x| gt + x)
                .ok_or(MusicalError::Invalid("rehearsal close"))?;
            let text = decode_xml_text(&body[gt..en]);
            if !text.is_empty() {
                out.marks.push(MusicalMark {
                    measure: measure_no,
                    count,
                    text,
                    kind: MarkKind::Rehearsal,
                });
            }
            from = en + 12;
        }
        count += f32::from(meter.0) * 4.0 / f32::from(meter.1) / unit.quarters() as f32;
    }
    if out.measures.is_empty() {
        return Err(MusicalError::Invalid("no selected measures"));
    }
    out.total_counts = count.ceil().min(u32::MAX as f32) as u32;
    Ok(MusicalImport {
        timeline: out,
        warnings,
    })
}

fn extract_mxl(bytes: &[u8], opts: &MusicalImportOptions) -> Result<Vec<u8>, MusicalError> {
    const MAX_ENTRIES: usize = 64;
    const MAX_EXPANDED: u64 = 64 << 20;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| MusicalError::Invalid("invalid MXL ZIP"))?;
    if archive.len() > MAX_ENTRIES {
        return Err(MusicalError::Limit("MXL entry count"));
    }
    let mut names = BTreeSet::new();
    let mut candidates = Vec::new();
    let mut expanded = 0_u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| MusicalError::Invalid("MXL entry"))?;
        if file.encrypted() {
            return Err(MusicalError::Invalid("encrypted MXL is unsupported"));
        }
        let name = file.name().to_owned();
        validate_mxl_path(&name)?;
        if !names.insert(name.clone()) {
            return Err(MusicalError::Invalid("duplicate MXL entry"));
        }
        expanded = expanded
            .checked_add(file.size())
            .ok_or(MusicalError::Limit("MXL expanded size"))?;
        if expanded > MAX_EXPANDED {
            return Err(MusicalError::Limit("MXL expanded size"));
        }
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".musicxml")
            || (lower.ends_with(".xml") && lower != "meta-inf/container.xml")
        {
            candidates.push(name);
        }
    }
    let root = if names.contains("META-INF/container.xml") {
        let container = read_zip_entry(&mut archive, "META-INF/container.xml", 1 << 20)?;
        let text = std::str::from_utf8(&container)
            .map_err(|_| MusicalError::Invalid("container.xml encoding"))?;
        if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
            return Err(MusicalError::Invalid("DTD/entities are forbidden"));
        }
        let tag = find_tag(text, 0, "rootfile")
            .and_then(|at| text[at..].find('>').map(|end| &text[at..at + end + 1]));
        let path = tag
            .and_then(|tag| attr(tag, "full-path"))
            .ok_or(MusicalError::Invalid("MXL rootfile missing"))?;
        validate_mxl_path(path)?;
        path.to_owned()
    } else {
        candidates.sort();
        candidates
            .into_iter()
            .next()
            .ok_or(MusicalError::Invalid("MXL score missing"))?
    };
    if !names.contains(&root) {
        return Err(MusicalError::Invalid("MXL rootfile not found"));
    }
    read_zip_entry(&mut archive, &root, opts.max_bytes as usize)
}

fn validate_mxl_path(path: &str) -> Result<(), MusicalError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.contains(':')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        Err(MusicalError::Invalid("unsafe MXL path"))
    } else {
        Ok(())
    }
}

fn read_zip_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    limit: usize,
) -> Result<Vec<u8>, MusicalError> {
    let mut file = archive
        .by_name(name)
        .map_err(|_| MusicalError::Invalid("MXL entry missing"))?;
    if file.encrypted() {
        return Err(MusicalError::Invalid("encrypted MXL is unsupported"));
    }
    if file.size() > limit as u64 {
        return Err(MusicalError::Limit("MXL entry size"));
    }
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| MusicalError::Invalid("MXL decompression"))?;
    if bytes.len() > limit {
        return Err(MusicalError::Limit("MXL entry size"));
    }
    Ok(bytes)
}

fn find_tag(s: &str, from: usize, name: &str) -> Option<usize> {
    let pat = format!("<{name}");
    let mut at = from;
    while let Some(relative) = s[at..].find(&pat) {
        let i = at + relative;
        if s.as_bytes()
            .get(i + pat.len())
            .is_some_and(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/')
        {
            return Some(i);
        }
        at = i + pat.len();
    }
    None
}
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let st = tag.find(&needle)? + needle.len();
    let en = tag[st..].find('"')? + st;
    Some(&tag[st..en])
}
fn tag_text<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let a = format!("<{name}>");
    let b = format!("</{name}>");
    let st = s.find(&a)? + a.len();
    let en = s[st..].find(&b)? + st;
    Some(s[st..en].trim())
}
fn decode_xml_text(s: &str) -> String {
    s.trim()
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}
fn take<'a>(b: &'a [u8], p: &mut usize, n: usize) -> Result<&'a [u8], MusicalError> {
    let e = p
        .checked_add(n)
        .filter(|&x| x <= b.len())
        .ok_or(MusicalError::Invalid("unexpected EOF"))?;
    let r = &b[*p..e];
    *p = e;
    Ok(r)
}
fn take_to<'a>(b: &'a [u8], p: &mut usize, n: usize, end: usize) -> Result<&'a [u8], MusicalError> {
    if p.checked_add(n).is_none_or(|x| x > end) {
        return Err(MusicalError::Invalid("event exceeds track"));
    }
    take(b, p, n)
}
fn be16(b: &[u8], p: &mut usize) -> Result<u16, MusicalError> {
    let x = take(b, p, 2)?;
    Ok(u16::from_be_bytes([x[0], x[1]]))
}
fn be32(b: &[u8], p: &mut usize) -> Result<u32, MusicalError> {
    let x = take(b, p, 4)?;
    Ok(u32::from_be_bytes([x[0], x[1], x[2], x[3]]))
}
fn vlq(b: &[u8], p: &mut usize, end: usize) -> Result<u64, MusicalError> {
    let mut v = 0u64;
    for _ in 0..4 {
        if *p >= end {
            return Err(MusicalError::Invalid("VLQ"));
        }
        let x = b[*p];
        *p += 1;
        v = (v << 7) | u64::from(x & 0x7f);
        if x & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(MusicalError::Invalid("VLQ too long"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn midi_tempo_meter_marker() {
        let b=b"MThd\0\0\0\x06\0\x01\0\x01\x01\xe0MTrk\0\0\0\x19\0\xff\x51\x03\x07\xa1\x20\0\xff\x58\x04\x04\x02\x18\x08\x83\x60\xff\x06\x01A\0\xff\x2f\0";
        let x = import_midi(b, &Default::default()).unwrap();
        assert_eq!(x.timeline.tempo.bpm_at(0.0), 120.0);
        assert_eq!(x.timeline.meters[0].denominator, 4);
        assert_eq!(x.timeline.marks[0].count, 1.0);
    }
    #[test]
    fn musicxml_extracts_timeline() {
        let b=br#"<score-partwise><part><measure number="1"><attributes><time><beats>6</beats><beat-type>8</beat-type></time></attributes><direction><sound tempo="180"/><direction-type><rehearsal>A &amp; B</rehearsal></direction-type></direction></measure><measure number="2"></measure></part></score-partwise>"#;
        let x = import_musicxml(b, &Default::default()).unwrap();
        assert_eq!(x.timeline.measures.len(), 2);
        assert_eq!(x.timeline.tempo.bpm_at(0.0), 120.0);
        assert_eq!(x.timeline.marks[0].text, "A & B");
        assert_eq!(x.timeline.measures[1].count, 2.0);
    }
    #[test]
    fn hostile_inputs_are_bounded() {
        let o = MusicalImportOptions {
            max_bytes: 4,
            ..Default::default()
        };
        assert!(matches!(
            import_midi(b"12345", &o),
            Err(MusicalError::TooLarge { .. })
        ));
        assert!(import_musicxml(b"<!DOCTYPE x><score-partwise/>", &Default::default()).is_err());
    }
    fn mxl(root: &str) -> Vec<u8> {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer
            .start_file("META-INF/container.xml", options)
            .unwrap();
        write!(
            writer,
            "<container><rootfiles><rootfile full-path=\"{root}\"/></rootfiles></container>"
        )
        .unwrap();
        writer.start_file("score/main.musicxml", options).unwrap();
        writer.write_all(br#"<score-partwise><part><measure number="1"><direction><sound tempo="144"/><direction-type><rehearsal>Intro</rehearsal></direction-type></direction></measure></part></score-partwise>"#).unwrap();
        writer.finish().unwrap().into_inner()
    }
    #[test]
    fn compressed_mxl_resolves_container_rootfile() {
        let x = import_musicxml(&mxl("score/main.musicxml"), &Default::default()).unwrap();
        assert_eq!(x.timeline.tempo.bpm_at(0.0), 144.0);
        assert_eq!(x.timeline.marks[0].text, "Intro");
    }
    #[test]
    fn compressed_mxl_rejects_traversal_and_missing_root() {
        assert!(matches!(
            import_musicxml(&mxl("../score/main.musicxml"), &Default::default()),
            Err(MusicalError::Invalid("unsafe MXL path"))
        ));
        assert!(import_musicxml(&mxl("other.musicxml"), &Default::default()).is_err());
    }
}
