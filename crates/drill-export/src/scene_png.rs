//! A printable picture of one scene.
//!
//! The audience side is the bottom edge, matching the field on screen.
//! The file is an uncompressed PNG so saving a scene does not add a crate.

use drill_core::Document;

const MARGIN: u32 = 28;
const IMAGE_WIDTH: u32 = 640;
const DOT: [u8; 3] = [30, 90, 180];
const LABEL: [u8; 3] = [24, 49, 84];
const FIELD: [u8; 3] = [232, 242, 255];
const LINE: [u8; 3] = [186, 210, 235];
const CENTER: [u8; 3] = [76, 163, 255];
const AUDIENCE: [u8; 3] = [47, 111, 196];

/// RGB picture of `set_index`. An unknown scene still returns a blank field.
pub fn scene_diagram_png(document: &Document, set_index: usize) -> Vec<u8> {
    let grid_w = positive(document.grid.width);
    let grid_h = positive(document.grid.height);
    let inner_w = IMAGE_WIDTH - MARGIN * 2;
    let inner_h = ((inner_w as f32) * (grid_h / grid_w))
        .round()
        .clamp(120.0, 420.0) as u32;
    let height = inner_h + MARGIN * 2;
    let mut image = Image::new(IMAGE_WIDTH, height, [255, 255, 255]);
    let left = MARGIN as i32;
    let top = MARGIN as i32;
    let right = (IMAGE_WIDTH - MARGIN) as i32;
    let bottom = (height - MARGIN) as i32;
    image.fill_rect(left, top, right, bottom, FIELD);

    let mut x = 0.0;
    while x <= grid_w {
        let px = map_x(x, grid_w, left, right);
        image.vline(px, top, bottom, LINE);
        x += positive(document.grid.major_line_interval);
    }
    for hash in &document.grid.hashes {
        let py = map_y(hash.position, grid_h, top, bottom);
        image.hline(left, right, py, LINE);
    }
    image.vline(
        map_x(grid_w * 0.5, grid_w, left, right),
        top,
        bottom,
        CENTER,
    );
    image.hline(left, right, bottom - 1, AUDIENCE);
    image.hline(left, right, bottom - 2, AUDIENCE);

    let positions = document
        .sets
        .get(set_index)
        .map(|set| set.positions.as_slice())
        .unwrap_or(&[]);
    for (index, point) in positions.iter().enumerate() {
        let px = map_x(point.x, grid_w, left, right);
        let py = map_y(point.y, grid_h, top, bottom);
        image.fill_circle(px, py, 5, DOT);
        let caption = diagram_label(document, index);
        image.text(px + 7, py - 8, &caption, LABEL);
    }
    encode_png(&image)
}

fn diagram_label(document: &Document, index: usize) -> String {
    let raw = document
        .performers
        .get(index)
        .map(|performer| performer.label.trim())
        .filter(|label| !label.is_empty() && label.chars().all(|ch| glyph(ch).is_some()));
    match raw {
        Some(label) => label.chars().take(8).collect(),
        None => (index + 1).to_string(),
    }
}

fn positive(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        1.0
    }
}

fn map_x(x: f32, grid_w: f32, left: i32, right: i32) -> i32 {
    let t = (x / grid_w).clamp(0.0, 1.0);
    left + (t * (right - left) as f32).round() as i32
}

fn map_y(y: f32, grid_h: f32, top: i32, bottom: i32) -> i32 {
    let t = (y / grid_h).clamp(0.0, 1.0);
    bottom - (t * (bottom - top) as f32).round() as i32
}

struct Image {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
}

impl Image {
    fn new(width: u32, height: u32, fill: [u8; 3]) -> Self {
        let mut rgb = vec![0; width as usize * height as usize * 3];
        for pixel in rgb.as_chunks_mut::<3>().0 {
            pixel.copy_from_slice(&fill);
        }
        Self { width, height, rgb }
    }

    fn set(&mut self, x: i32, y: i32, color: [u8; 3]) {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height {
            return;
        }
        let index = (y as u32 * self.width + x as u32) as usize * 3;
        self.rgb[index..index + 3].copy_from_slice(&color);
    }

    fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: [u8; 3]) {
        for y in y0..y1 {
            for x in x0..x1 {
                self.set(x, y, color);
            }
        }
    }

    fn hline(&mut self, x0: i32, x1: i32, y: i32, color: [u8; 3]) {
        for x in x0..x1 {
            self.set(x, y, color);
        }
    }

    fn vline(&mut self, x: i32, y0: i32, y1: i32, color: [u8; 3]) {
        for y in y0..y1 {
            self.set(x, y, color);
        }
    }

    fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, color: [u8; 3]) {
        let limit = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= limit {
                    self.set(cx + dx, cy + dy, color);
                }
            }
        }
    }

    fn text(&mut self, x: i32, y: i32, text: &str, color: [u8; 3]) {
        let mut cursor = x;
        for ch in text.chars() {
            if let Some(rows) = glyph(ch) {
                for (row, bits) in rows.iter().enumerate() {
                    for col in 0..5 {
                        if bits & (1 << (4 - col)) != 0 {
                            self.set(cursor + col, y + row as i32, color);
                        }
                    }
                }
            }
            cursor += 6;
        }
    }
}

fn encode_png(image: &Image) -> Vec<u8> {
    let width = image.width;
    let height = image.height;
    let stride = width as usize * 3;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for row in 0..height as usize {
        raw.push(0);
        let start = row * stride;
        raw.extend_from_slice(&image.rgb[start..start + stride]);
    }
    let mut png = Vec::with_capacity(raw.len() + 64);
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &zlib_store(&raw));
    write_chunk(&mut png, b"IEND", &[]);
    png
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 16);
    out.extend_from_slice(&[0x78, 0x01]);
    let mut offset = 0;
    loop {
        let remaining = data.len() - offset;
        let chunk = remaining.min(65_535);
        let last = offset + chunk == data.len();
        out.push(if last { 1 } else { 0 });
        let len = u16::try_from(chunk).unwrap_or(u16::MAX);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&data[offset..offset + chunk]);
        offset += chunk;
        if last {
            break;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// 5×7 glyphs. Low 5 bits, high bit is the left column. Digits, A–Z, hyphen, space.
fn glyph(ch: char) -> Option<[u8; 7]> {
    const GLYPHS: [[u8; 7]; 38] = [
        [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
        [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    ];
    let index = match ch {
        '0'..='9' => ch as usize - '0' as usize,
        'A'..='Z' => 10 + ch as usize - 'A' as usize,
        'a'..='z' => 10 + ch as usize - 'a' as usize,
        '-' => 36,
        ' ' => 37,
        _ => return None,
    };
    GLYPHS.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use drill_core::Point;

    #[test]
    fn crc32_matches_the_png_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn adler32_matches_a_known_string() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn scene_picture_is_a_png_and_moves_when_a_person_moves() {
        let mut document = Document::demo(1, 1);
        document.sets[0].positions[0] = Point { x: 50.0, y: 26.0 };
        document.sets[1].positions[0] = document.sets[0].positions[0];
        let first = scene_diagram_png(&document, 0);
        let (width, height, rgb) = decode_rgb(&first);
        assert_eq!(width, IMAGE_WIDTH);
        assert!(height > MARGIN * 2);
        assert!(rgb.as_chunks::<3>().0.contains(&DOT));
        assert!(rgb.as_chunks::<3>().0.contains(&FIELD));

        document.sets[0].positions[0] = Point { x: 8.0, y: 8.0 };
        let moved = scene_diagram_png(&document, 0);
        assert_ne!(first, moved);
        assert!(moved.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn a_japanese_name_falls_back_to_the_person_number() {
        let mut named = Document::demo(1, 1);
        named.performers[0].label = "フルート".into();
        let mut numbered = named.clone();
        numbered.performers[0].label = "1".into();
        assert_eq!(
            scene_diagram_png(&named, 0),
            scene_diagram_png(&numbered, 0)
        );
    }

    fn decode_rgb(png: &[u8]) -> (u32, u32, Vec<u8>) {
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let mut offset = 8;
        let mut width = 0;
        let mut height = 0;
        let mut idat = Vec::new();
        while offset + 12 <= png.len() {
            let len = u32::from_be_bytes(png[offset..offset + 4].try_into().unwrap()) as usize;
            let kind = &png[offset + 4..offset + 8];
            let data = &png[offset + 8..offset + 8 + len];
            let expected = crc32(&png[offset + 4..offset + 8 + len]);
            let stored =
                u32::from_be_bytes(png[offset + 8 + len..offset + 12 + len].try_into().unwrap());
            assert_eq!(stored, expected, "chunk crc");
            if kind == b"IHDR" {
                width = u32::from_be_bytes(data[0..4].try_into().unwrap());
                height = u32::from_be_bytes(data[4..8].try_into().unwrap());
                assert_eq!(data[8], 8);
                assert_eq!(data[9], 2);
            } else if kind == b"IDAT" {
                idat.extend_from_slice(data);
            } else if kind == b"IEND" {
                break;
            }
            offset += 12 + len;
        }
        assert_eq!(&idat[0..2], &[0x78, 0x01]);
        let mut raw = Vec::new();
        let mut index = 2;
        loop {
            let header = idat[index];
            index += 1;
            let len = u16::from_le_bytes([idat[index], idat[index + 1]]) as usize;
            index += 2;
            let nlen = u16::from_le_bytes([idat[index], idat[index + 1]]);
            index += 2;
            assert_eq!(nlen, !(len as u16));
            raw.extend_from_slice(&idat[index..index + len]);
            index += len;
            if header & 1 == 1 {
                break;
            }
        }
        let checksum = u32::from_be_bytes(idat[index..index + 4].try_into().unwrap());
        assert_eq!(checksum, adler32(&raw));
        let stride = width as usize * 3 + 1;
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        for row in 0..height as usize {
            assert_eq!(raw[row * stride], 0);
            let start = row * stride + 1;
            rgb.extend_from_slice(&raw[start..start + width as usize * 3]);
        }
        (width, height, rgb)
    }
}
