//! Bounded font outline extraction for Formation Text.
use drill_core::{Point, shapes::ShapeSpec};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
};

#[derive(Clone, Copy, Debug)]
pub struct TextOutlineOptions {
    pub origin: Point,
    pub height: f32,
    pub letter_spacing: f32,
    pub max_chars: usize,
    pub max_contours: usize,
    pub max_vertices: usize,
    pub curve_segments: u8,
}
impl Default for TextOutlineOptions {
    fn default() -> Self {
        Self {
            origin: Point { x: 10.0, y: 20.0 },
            height: 20.0,
            letter_spacing: 0.08,
            max_chars: 32,
            max_contours: 256,
            max_vertices: 16_384,
            curve_segments: 8,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextOutlineWarning {
    MissingGlyph(char),
    WhitespaceOnly,
}
#[derive(Clone, Debug)]
pub struct TextOutline {
    pub shape: ShapeSpec,
    pub warnings: Vec<TextOutlineWarning>,
    pub width: f32,
    pub height: f32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextOutlineError {
    InvalidFont,
    Empty,
    Limit(&'static str),
    InvalidOptions,
    Outline,
}
impl std::fmt::Display for TextOutlineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFont => f.write_str("invalid formation text font"),
            Self::Empty => f.write_str("formation text is empty"),
            Self::Limit(v) => write!(f, "formation text exceeds safety limit: {v}"),
            Self::InvalidOptions => f.write_str("formation text dimensions are invalid"),
            Self::Outline => f.write_str("glyph outline could not be read"),
        }
    }
}
impl std::error::Error for TextOutlineError {}

pub fn outline_text(
    font_bytes: &[u8],
    text: &str,
    opts: TextOutlineOptions,
) -> Result<TextOutline, TextOutlineError> {
    if text.chars().count() > opts.max_chars {
        return Err(TextOutlineError::Limit("characters"));
    }
    if text.is_empty() {
        return Err(TextOutlineError::Empty);
    }
    if !opts.height.is_finite()
        || opts.height <= 0.0
        || !opts.letter_spacing.is_finite()
        || opts.curve_segments == 0
        || opts.curve_segments > 32
        || opts.max_vertices == 0
        || opts.max_contours == 0
    {
        return Err(TextOutlineError::InvalidOptions);
    }
    let font = FontRef::new(font_bytes).map_err(|_| TextOutlineError::InvalidFont)?;
    let outlines = font.outline_glyphs();
    let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let mut pen = Collector::new(opts);
    let mut cursor = 0.0;
    let mut warnings = Vec::new();
    for ch in text.chars() {
        if ch == '\n' {
            return Err(TextOutlineError::Limit("single line only"));
        }
        let Some(gid) = font.charmap().map(ch) else {
            if !ch.is_whitespace() {
                warnings.push(TextOutlineWarning::MissingGlyph(ch))
            }
            cursor += metrics
                .advance_width(font.charmap().map(' ').unwrap_or_default())
                .unwrap_or(500.0);
            continue;
        };
        if let Some(glyph) = outlines.get(gid) {
            pen.offset = cursor;
            glyph
                .draw(
                    DrawSettings::unhinted(Size::unscaled(), LocationRef::default()),
                    &mut pen,
                )
                .map_err(|_| TextOutlineError::Outline)?;
        }
        cursor += metrics.advance_width(gid).unwrap_or(500.0) * (1.0 + opts.letter_spacing);
        if pen.vertices > opts.max_vertices || pen.contours.len() > opts.max_contours {
            return Err(TextOutlineError::Limit("outline complexity"));
        }
    }
    pen.finish();
    if pen.contours.is_empty() {
        warnings.push(TextOutlineWarning::WhitespaceOnly);
        return Err(TextOutlineError::Empty);
    }
    let (min, max) = bounds(&pen.contours);
    let raw_h = (max.y - min.y).max(1.0);
    let scale = opts.height / raw_h;
    for contour in &mut pen.contours {
        for p in contour {
            p.x = opts.origin.x + (p.x - min.x) * scale;
            p.y = opts.origin.y + (max.y - p.y) * scale;
        }
    }
    let width = (max.x - min.x) * scale;
    let shape = ShapeSpec::Text {
        contours: pen.contours,
    };
    shape
        .validate()
        .map_err(|_| TextOutlineError::Limit("ShapeSpec"))?;
    Ok(TextOutline {
        shape,
        warnings,
        width,
        height: opts.height,
    })
}
struct Collector {
    contours: Vec<Vec<Point>>,
    current: Vec<Point>,
    offset: f32,
    last: Point,
    opts: TextOutlineOptions,
    vertices: usize,
}
impl Collector {
    fn new(opts: TextOutlineOptions) -> Self {
        Self {
            contours: Vec::new(),
            current: Vec::new(),
            offset: 0.0,
            last: Point::default(),
            opts,
            vertices: 0,
        }
    }
    fn add(&mut self, p: Point) {
        if self.vertices < self.opts.max_vertices + 1 {
            self.current.push(p);
            self.vertices += 1
        }
        self.last = p
    }
    fn finish(&mut self) {
        if self.current.len() > 1 {
            self.contours.push(std::mem::take(&mut self.current))
        } else {
            self.current.clear()
        }
    }
}
impl OutlinePen for Collector {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        self.add(Point {
            x: x + self.offset,
            y,
        })
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.add(Point {
            x: x + self.offset,
            y,
        })
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let a = self.last;
        for i in 1..=self.opts.curve_segments {
            let t = f32::from(i) / f32::from(self.opts.curve_segments);
            let u = 1.0 - t;
            self.add(Point {
                x: u * u * a.x + 2.0 * u * t * (cx + self.offset) + t * t * (x + self.offset),
                y: u * u * a.y + 2.0 * u * t * cy + t * t * y,
            })
        }
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let a = self.last;
        for i in 1..=self.opts.curve_segments {
            let t = f32::from(i) / f32::from(self.opts.curve_segments);
            let u = 1.0 - t;
            self.add(Point {
                x: u.powi(3) * a.x
                    + 3.0 * u * u * t * (cx0 + self.offset)
                    + 3.0 * u * t * t * (cx1 + self.offset)
                    + t.powi(3) * (x + self.offset),
                y: u.powi(3) * a.y + 3.0 * u * u * t * cy0 + 3.0 * u * t * t * cy1 + t.powi(3) * y,
            })
        }
    }
    fn close(&mut self) {
        self.finish()
    }
}
fn bounds(c: &[Vec<Point>]) -> (Point, Point) {
    let mut min = Point {
        x: f32::INFINITY,
        y: f32::INFINITY,
    };
    let mut max = Point {
        x: f32::NEG_INFINITY,
        y: f32::NEG_INFINITY,
    };
    for p in c.iter().flatten() {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y)
    }
    (min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    const FONT: &[u8] = include_bytes!("../../../assets/NotoSansJP.ttf");
    #[test]
    fn japanese_and_latin_become_bounded_shape() {
        let x = outline_text(FONT, "Drill 響", Default::default()).unwrap();
        assert!(matches!(x.shape, ShapeSpec::Text { .. }));
        assert!(x.width > 0.0 && x.height == 20.0);
        x.shape.validate().unwrap();
    }
    #[test]
    fn deterministic_and_missing_fallback_explicit() {
        let a = outline_text(FONT, "A😀", Default::default()).unwrap();
        let b = outline_text(FONT, "A😀", Default::default()).unwrap();
        assert_eq!(a.shape, b.shape);
        assert!(a.warnings.contains(&TextOutlineWarning::MissingGlyph('😀')));
    }
    #[test]
    fn hostile_limits_and_fonts_rejected() {
        let o = TextOutlineOptions {
            max_chars: 1,
            ..Default::default()
        };
        assert!(matches!(
            outline_text(FONT, "AB", o),
            Err(TextOutlineError::Limit(_))
        ));
        assert_eq!(
            outline_text(b"bad", "A", Default::default()).unwrap_err(),
            TextOutlineError::InvalidFont
        );
    }
}
