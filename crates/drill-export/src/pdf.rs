//! Small deterministic PDF 1.4 writer for printable DrillForge reports.
//!
//! It deliberately writes no timestamps or random document IDs. Field diagrams
//! consume the exact same `DisplayList` as the interactive and video renderers.

use crate::page::{PageItem, PrintSettings, ReportDocument, paginate};
use drill_core::Document;
use drill_jobs::{Job, JobErrorCode, JobFailure, JobKind};
use drill_render::{
    BuildScratch, DisplayList, DrawCmd, RenderOptions, Scene, Theme, Vec2, Viewport, build_field_2d,
};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io;
use std::path::Path;
use std::path::PathBuf;

const PT_PER_MM: f32 = 72.0 / 25.4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfSummary {
    pub pages: usize,
    pub bytes: usize,
}

#[derive(Clone, Debug)]
pub struct PdfExportRequest {
    pub document: Document,
    pub report: ReportDocument,
    pub settings: PrintSettings,
    pub output: PathBuf,
    pub underlay: Option<Vec<u8>>,
}

/// Runs layout and PDF generation away from the UI thread and atomically
/// installs the completed file. Existing files are deliberately refused.
pub fn spawn_pdf_export(request: PdfExportRequest) -> Job<PdfSummary> {
    Job::spawn_typed(JobKind::ExportText, move |progress| {
        progress.set(0.05);
        if progress.is_cancelled() {
            return Err(JobFailure::new(JobErrorCode::Cancelled));
        }
        let bytes = render_pdf_with_underlay(
            &request.document,
            &request.report,
            &request.settings,
            request.underlay.as_deref(),
        )
        .map_err(|_| JobFailure::new(JobErrorCode::Validation))?;
        progress.set(0.8);
        if request.output.exists() {
            return Err(JobFailure::new(JobErrorCode::Busy));
        }
        let parent = request.output.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|_| JobFailure::new(JobErrorCode::Io))?;
        let name = request
            .output
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("report.pdf");
        let staged = parent.join(format!(".{name}.{}.partial", std::process::id()));
        std::fs::write(&staged, &bytes).map_err(|_| JobFailure::new(JobErrorCode::Io))?;
        if progress.is_cancelled() {
            let _ = std::fs::remove_file(&staged);
            return Err(JobFailure::new(JobErrorCode::Cancelled));
        }
        drill_project::install_file(&staged, &request.output, None)
            .map_err(|_| JobFailure::new(JobErrorCode::Io))?;
        progress.set(1.0);
        Ok(PdfSummary {
            pages: paginate(&request.report, &request.settings).len(),
            bytes: bytes.len(),
        })
    })
}

pub fn render_pdf(doc: &Document, report: &ReportDocument, settings: &PrintSettings) -> Vec<u8> {
    render_pdf_with_underlay(doc, report, settings, None).unwrap_or_else(|_| Vec::new())
}

pub fn render_pdf_with_underlay(
    doc: &Document,
    report: &ReportDocument,
    settings: &PrintSettings,
    encoded: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let underlay = if let Some(model) = doc.underlay.as_ref().filter(|u| {
        u.placement.render_policy == drill_core::underlay::UnderlayRenderPolicy::EditorAnd2dExport
            && u.placement.visible
    }) {
        let bytes = encoded.ok_or_else(|| "underlay-missing".to_owned())?;
        let decoded = drill_interop::underlay::decode_underlay(
            bytes,
            drill_interop::underlay::UnderlayLimits::default(),
        )
        .map_err(|_| "underlay-invalid".to_owned())?;
        if decoded
            .source_hash
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
            != model.content_hash
        {
            return Err("underlay-hash".into());
        }
        Some((decoded, model.placement.clone()))
    } else {
        None
    };
    let pages = paginate(report, settings);
    let (width_mm, height_mm) = settings.dimensions_mm();
    let width = width_mm * PT_PER_MM;
    let height = height_mm * PT_PER_MM;
    let mut contents = Vec::with_capacity(pages.len());
    let mut font = PdfFont::new();
    for page in &pages {
        let mut stream = String::new();
        let mut y = height - settings.margins.top_mm * PT_PER_MM;
        pdf_text(
            &mut stream,
            &mut font,
            settings.margins.left_mm * PT_PER_MM,
            y,
            13.0,
            &report.title,
        );
        pdf_text(
            &mut stream,
            &mut font,
            width - settings.margins.right_mm * PT_PER_MM - 45.0,
            18.0,
            8.0,
            &format!("{} / {}", page.number, pages.len()),
        );
        y -= 20.0;
        for item in &page.items {
            match item {
                PageItem::Heading(value) => {
                    pdf_text(
                        &mut stream,
                        &mut font,
                        settings.margins.left_mm * PT_PER_MM,
                        y,
                        12.0,
                        value,
                    );
                    y -= 24.0;
                }
                PageItem::Text(value) => {
                    pdf_text(
                        &mut stream,
                        &mut font,
                        settings.margins.left_mm * PT_PER_MM,
                        y,
                        9.0,
                        value,
                    );
                    y -= 16.0;
                }
                PageItem::Table { columns, rows } => {
                    let left = settings.margins.left_mm * PT_PER_MM;
                    let available = width - left - settings.margins.right_mm * PT_PER_MM;
                    let col_w = available / columns.len().max(1) as f32;
                    table_row(&mut stream, &mut font, left, y, col_w, columns, true);
                    y -= 17.0;
                    for row in rows {
                        table_row(&mut stream, &mut font, left, y, col_w, row, false);
                        y -= 17.0;
                    }
                    y -= 5.0;
                }
                PageItem::FieldDiagram { set_index } => {
                    if let Some(set) = doc.sets.get(*set_index) {
                        let diagram_h =
                            (y - settings.margins.bottom_mm * PT_PER_MM - 8.0).max(80.0);
                        let diagram_w = width
                            - (settings.margins.left_mm + settings.margins.right_mm) * PT_PER_MM;
                        let mut list = DisplayList::new();
                        build_field_2d(
                            &Scene {
                                document: doc,
                                positions: &set.positions,
                                viewport: Viewport {
                                    size: Vec2 {
                                        x: diagram_w,
                                        y: diagram_h,
                                    },
                                    ui_scale: 1.0,
                                },
                                options: &RenderOptions::default(),
                                theme: &Theme::PRINT_LIGHT,
                            },
                            &mut BuildScratch,
                            &mut list,
                        );
                        display_list(
                            &mut stream,
                            &mut font,
                            &list,
                            settings.margins.left_mm * PT_PER_MM,
                            y - diagram_h,
                            underlay.as_ref(),
                        );
                        y -= diagram_h + 8.0;
                    }
                }
                PageItem::PageBreak => {}
            }
        }
        contents.push(stream.into_bytes());
    }
    Ok(assemble_pdf(width, height, &contents, &font))
}

pub fn save_pdf(
    path: &Path,
    doc: &Document,
    report: &ReportDocument,
    settings: &PrintSettings,
) -> io::Result<PdfSummary> {
    let bytes = render_pdf(doc, report, settings);
    std::fs::write(path, &bytes)?;
    Ok(PdfSummary {
        pages: paginate(report, settings).len(),
        bytes: bytes.len(),
    })
}

fn table_row(
    out: &mut String,
    font: &mut PdfFont,
    left: f32,
    y: f32,
    col_w: f32,
    cells: &[String],
    header: bool,
) {
    if header {
        let _ = writeln!(
            out,
            "0.92 g {:.2} {:.2} {:.2} 15 re f 0 g",
            left,
            y - 4.0,
            col_w * cells.len() as f32
        );
    }
    for (i, cell) in cells.iter().enumerate() {
        pdf_text(
            out,
            font,
            left + i as f32 * col_w + 2.0,
            y,
            if header { 8.0 } else { 7.0 },
            &truncate(cell, (col_w / 4.0) as usize),
        );
    }
    let _ = writeln!(
        out,
        "0.7 G {:.2} {:.2} m {:.2} {:.2} l S",
        left,
        y - 5.0,
        left + col_w * cells.len() as f32,
        y - 5.0
    );
}

fn truncate(value: &str, max: usize) -> String {
    let mut s: String = value.chars().take(max.max(1)).collect();
    if value.chars().count() > max {
        s.push('>');
    }
    s
}

fn pdf_text(out: &mut String, font: &mut PdfFont, x: f32, y: f32, size: f32, value: &str) {
    let encoded = font.encode(value);
    let _ = writeln!(
        out,
        "BT /F1 {:.2} Tf {:.2} {:.2} Td <{}> Tj ET",
        size, x, y, encoded
    );
}

fn display_list(
    out: &mut String,
    font: &mut PdfFont,
    list: &DisplayList,
    ox: f32,
    oy: f32,
    underlay: Option<&(
        drill_interop::underlay::DecodedUnderlay,
        drill_core::underlay::UnderlayPlacement,
    )>,
) {
    let height = list.viewport().size.y;
    let point = |p: Vec2| (ox + p.x, oy + height - p.y);
    for command in list.paint_order() {
        match *command {
            DrawCmd::FieldFill { rect, fill } => {
                let (x, y) = point(Vec2 {
                    x: rect.min.x,
                    y: rect.max.y,
                });
                color(out, fill, true);
                let _ = writeln!(
                    out,
                    "{x:.2} {y:.2} {:.2} {:.2} re f",
                    rect.max.x - rect.min.x,
                    rect.max.y - rect.min.y
                );
                if let Some((image, placement)) = underlay {
                    pdf_underlay(out, image, placement, ox, oy, height);
                }
            }
            DrawCmd::Line {
                a,
                b,
                width,
                color: c,
            } => {
                let (ax, ay) = point(a);
                let (bx, by) = point(b);
                color(out, c, false);
                let _ = writeln!(
                    out,
                    "{:.2} w {ax:.2} {ay:.2} m {bx:.2} {by:.2} l S",
                    width.max(0.1)
                );
            }
            DrawCmd::Dot {
                center,
                radius,
                fill,
                ..
            } => {
                let (x, y) = point(center);
                color(out, fill, true);
                let k = 0.552_284_8 * radius;
                let _ = writeln!(
                    out,
                    "{:.2} {:.2} m {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c f",
                    x + radius,
                    y,
                    x + radius,
                    y + k,
                    x + k,
                    y + radius,
                    x,
                    y + radius,
                    x - k,
                    y + radius,
                    x - radius,
                    y + k,
                    x - radius,
                    y,
                    x - radius,
                    y - k,
                    x - k,
                    y - radius,
                    x,
                    y - radius,
                    x + k,
                    y - radius,
                    x + radius,
                    y - k,
                    x + radius,
                    y
                );
            }
            DrawCmd::Text {
                at,
                text,
                size,
                color: c,
            } => {
                color(out, c, true);
                let (x, y) = point(at);
                pdf_text(out, font, x, y, size, list.text(text));
            }
        }
    }
}

fn pdf_underlay(
    out: &mut String,
    image: &drill_interop::underlay::DecodedUnderlay,
    p: &drill_core::underlay::UnderlayPlacement,
    ox: f32,
    oy: f32,
    height: f32,
) {
    let sx = image.width.div_ceil(128).max(1);
    let sy = image.height.div_ceil(128).max(1);
    let display_w = 1000.0_f32.min(height * 2.0) * p.scale_x;
    let display_h = height * p.scale_y;
    let cx = ox + 500.0_f32.min(display_w / p.scale_x.max(0.001)) + p.x / 100.0 * display_w;
    let cy = oy + height * (0.5 + p.y / 100.0);
    let (sin, cos) = (-p.rotation_radians).sin_cos();
    let rotate = |x: f32, y: f32| (cx + x * cos - y * sin, cy + x * sin + y * cos);
    for py in (0..image.height).step_by(sy as usize) {
        for px in (0..image.width).step_by(sx as usize) {
            let i = ((py * image.width + px) * 4) as usize;
            let alpha = image.rgba[i + 3] as f32 / 255.0 * p.opacity;
            if alpha <= 0.001 {
                continue;
            }
            let blend = |source: u8, field: u8| {
                (source as f32 * alpha + field as f32 * (1.0 - alpha)) / 255.0
            };
            let _ = writeln!(
                out,
                "{:.4} {:.4} {:.4} rg",
                blend(image.rgba[i], 25),
                blend(image.rgba[i + 1], 71),
                blend(image.rgba[i + 2], 45)
            );
            let x0 = px as f32 / image.width as f32 * display_w - display_w * 0.5;
            let x1 = (px + sx).min(image.width) as f32 / image.width as f32 * display_w
                - display_w * 0.5;
            let y0 = display_h * 0.5 - py as f32 / image.height as f32 * display_h;
            let y1 = display_h * 0.5
                - (py + sy).min(image.height) as f32 / image.height as f32 * display_h;
            let a = rotate(x0, y0);
            let b = rotate(x1, y0);
            let c = rotate(x1, y1);
            let d = rotate(x0, y1);
            let _ = writeln!(
                out,
                "{:.2} {:.2} m {:.2} {:.2} l {:.2} {:.2} l {:.2} {:.2} l h f",
                a.0, a.1, b.0, b.1, c.0, c.1, d.0, d.1
            );
        }
    }
}

fn color(out: &mut String, c: drill_render::Rgba, fill: bool) {
    let op = if fill { "rg" } else { "RG" };
    let _ = writeln!(
        out,
        "{:.3} {:.3} {:.3} {op}",
        f32::from(c.0) / 255.0,
        f32::from(c.1) / 255.0,
        f32::from(c.2) / 255.0
    );
}

const NOTO_SANS_JP: &[u8] = include_bytes!("../../../assets/NotoSansJP.ttf");

/// Encodes Unicode text as two-byte glyph IDs for an Identity-H Type0 font.
/// Keeping the used map ordered makes both `/W` and `ToUnicode` deterministic.
struct PdfFont {
    ttf: TrueType<'static>,
    used: BTreeMap<u16, char>,
}

impl PdfFont {
    fn new() -> Self {
        Self {
            ttf: TrueType::parse(NOTO_SANS_JP).expect("bundled NotoSansJP.ttf must be valid"),
            used: BTreeMap::new(),
        }
    }

    fn encode(&mut self, value: &str) -> String {
        let mut encoded = String::with_capacity(value.chars().count() * 4);
        for c in value.chars().filter(|c| !c.is_control()) {
            let glyph = self.ttf.glyph(c).unwrap_or(0);
            self.used.entry(glyph).or_insert(c);
            let _ = write!(encoded, "{glyph:04X}");
        }
        encoded
    }

    fn cid_font_object(&self) -> Vec<u8> {
        let mut widths = String::new();
        for glyph in self.used.keys() {
            let width = self.ttf.width(*glyph);
            let _ = write!(widths, " {glyph} [{width}]");
        }
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NotoSansJP /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 5 0 R /CIDToGIDMap /Identity /DW 1000 /W [{widths} ] >>"
        )
        .into_bytes()
    }

    fn descriptor_object(&self) -> Vec<u8> {
        let [x_min, y_min, x_max, y_max] = self.ttf.bbox_1000();
        format!(
            "<< /Type /FontDescriptor /FontName /NotoSansJP /Flags 4 /FontBBox [{x_min} {y_min} {x_max} {y_max}] /ItalicAngle 0 /Ascent {y_max} /Descent {y_min} /CapHeight {y_max} /StemV 80 /FontFile2 6 0 R >>"
        )
        .into_bytes()
    }

    fn to_unicode(&self) -> String {
        let mut result = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /DrillForge-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        let entries: Vec<_> = self.used.iter().collect();
        for chunk in entries.chunks(100) {
            let _ = writeln!(result, "{} beginbfchar", chunk.len());
            for (glyph, c) in chunk {
                let _ = writeln!(
                    result,
                    "<{glyph:04X}> <{}>",
                    c.encode_utf16(&mut [0; 2])
                        .iter()
                        .map(|unit| format!("{unit:04X}"))
                        .collect::<String>()
                );
            }
            result.push_str("endbfchar\n");
        }
        result.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        result
    }
}

fn stream_object(bytes: &[u8]) -> Vec<u8> {
    let mut stream = format!("<< /Length {} >>\nstream\n", bytes.len()).into_bytes();
    stream.extend_from_slice(bytes);
    stream.extend_from_slice(b"\nendstream");
    stream
}

fn font_file_stream(bytes: &[u8]) -> Vec<u8> {
    let mut stream = format!(
        "<< /Length {} /Length1 {} >>\nstream\n",
        bytes.len(),
        bytes.len()
    )
    .into_bytes();
    stream.extend_from_slice(bytes);
    stream.extend_from_slice(b"\nendstream");
    stream
}

/// Minimal, bounds-checked TrueType reader. Only standardized tables needed by
/// PDF embedding are interpreted; the original font program is embedded whole.
struct TrueType<'a> {
    cmap: &'a [u8],
    head: &'a [u8],
    hhea: &'a [u8],
    hmtx: &'a [u8],
}

impl<'a> TrueType<'a> {
    fn parse(bytes: &'a [u8]) -> Option<Self> {
        let count = usize::from(be_u16(bytes, 4)?);
        let table = |tag: &[u8; 4]| -> Option<&'a [u8]> {
            (0..count).find_map(|i| {
                let p = 12 + i * 16;
                if bytes.get(p..p + 4)? != tag {
                    return None;
                }
                let offset = usize::try_from(be_u32(bytes, p + 8)?).ok()?;
                let length = usize::try_from(be_u32(bytes, p + 12)?).ok()?;
                bytes.get(offset..offset.checked_add(length)?)
            })
        };
        Some(Self {
            cmap: table(b"cmap")?,
            head: table(b"head")?,
            hhea: table(b"hhea")?,
            hmtx: table(b"hmtx")?,
        })
    }

    fn glyph(&self, c: char) -> Option<u16> {
        let count = usize::from(be_u16(self.cmap, 2)?);
        let mut format4 = None;
        for i in 0..count {
            let p = 4 + i * 8;
            let platform = be_u16(self.cmap, p)?;
            let encoding = be_u16(self.cmap, p + 2)?;
            let offset = usize::try_from(be_u32(self.cmap, p + 4)?).ok()?;
            let sub = self.cmap.get(offset..)?;
            match be_u16(sub, 0)? {
                12 if platform == 3 && encoding == 10 => return glyph_format12(sub, c as u32),
                4 if platform == 3 && (encoding == 1 || encoding == 10) => format4 = Some(sub),
                _ => {}
            }
        }
        glyph_format4(format4?, u16::try_from(c as u32).ok()?)
    }

    fn width(&self, glyph: u16) -> u16 {
        let units = be_u16(self.head, 18).unwrap_or(1000).max(1);
        let metrics = be_u16(self.hhea, 34).unwrap_or(1).max(1);
        let index = glyph.min(metrics - 1);
        let raw = be_u16(self.hmtx, usize::from(index) * 4).unwrap_or(units);
        ((u32::from(raw) * 1000 + u32::from(units) / 2) / u32::from(units)) as u16
    }

    fn bbox_1000(&self) -> [i32; 4] {
        let units = i32::from(be_u16(self.head, 18).unwrap_or(1000).max(1));
        let scale = |offset| i32::from(be_i16(self.head, offset).unwrap_or(0)) * 1000 / units;
        [scale(36), scale(38), scale(40), scale(42)]
    }
}

fn glyph_format12(table: &[u8], code: u32) -> Option<u16> {
    let groups = usize::try_from(be_u32(table, 12)?).ok()?;
    let mut low = 0;
    let mut high = groups;
    while low < high {
        let mid = low + (high - low) / 2;
        let p = 16 + mid * 12;
        let start = be_u32(table, p)?;
        let end = be_u32(table, p + 4)?;
        if code < start {
            high = mid;
        } else if code > end {
            low = mid + 1;
        } else {
            return u16::try_from(be_u32(table, p + 8)?.checked_add(code - start)?).ok();
        }
    }
    None
}

fn glyph_format4(table: &[u8], code: u16) -> Option<u16> {
    let segments = usize::from(be_u16(table, 6)?) / 2;
    let ends = 14;
    let starts = ends + segments * 2 + 2;
    let deltas = starts + segments * 2;
    let ranges = deltas + segments * 2;
    for i in 0..segments {
        let end = be_u16(table, ends + i * 2)?;
        let start = be_u16(table, starts + i * 2)?;
        if code < start || code > end {
            continue;
        }
        let delta = be_i16(table, deltas + i * 2)? as u16;
        let range = be_u16(table, ranges + i * 2)?;
        if range == 0 {
            return Some(code.wrapping_add(delta));
        }
        let address = ranges + i * 2 + usize::from(range) + usize::from(code - start) * 2;
        let glyph = be_u16(table, address)?;
        return Some(if glyph == 0 {
            0
        } else {
            glyph.wrapping_add(delta)
        });
    }
    None
}

fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn be_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn assemble_pdf(width: f32, height: f32, contents: &[Vec<u8>], font: &PdfFont) -> Vec<u8> {
    let page_ids: Vec<usize> = (0..contents.len()).map(|i| 8 + i * 2).collect();
    let kids = page_ids
        .iter()
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [{kids}] /Count {} >>",
            contents.len()
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /NotoSansJP /Encoding /Identity-H /DescendantFonts [4 0 R] /ToUnicode 7 0 R >>".to_vec(),
        font.cid_font_object(),
        font.descriptor_object(),
        font_file_stream(NOTO_SANS_JP),
        stream_object(font.to_unicode().as_bytes()),
    ];
    for (i, content) in contents.iter().enumerate() {
        let content_id = 9 + i * 2;
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width:.2} {height:.2}] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>").into_bytes());
        objects.push(stream_object(content));
    }
    let mut out = b"%PDF-1.4\n%DFRG\n".to_vec();
    let mut offsets = vec![0];
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(obj);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report;
    use std::collections::HashMap;
    #[test]
    fn pdf_is_valid_shape_and_deterministic() {
        let d = Document::demo(2, 3);
        let r = report::set_charts(&d, &[]);
        let a = render_pdf(&d, &r, &PrintSettings::default());
        let b = render_pdf(&d, &r, &PrintSettings::default());
        assert_eq!(a, b);
        assert!(a.starts_with(b"%PDF-1.4"));
        assert!(a.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&a);
        assert_eq!(text.matches("/Type /Page ").count(), d.sets.len());
    }
    #[test]
    fn drill_book_pdf_paginates() {
        let d = Document::demo(3, 4);
        let r = report::performer_drill_book(&d, &[]);
        let bytes = render_pdf(&d, &r, &PrintSettings::default());
        assert!(bytes.len() > 1000);
    }

    #[test]
    fn embeds_unicode_font_and_extracts_japanese_text() {
        let d = Document::demo(1, 2);
        let phrase = "第12セット・開始位置 𠮷田";
        let report = ReportDocument {
            title: "ドリル図面".into(),
            items: vec![
                PageItem::Heading(phrase.into()),
                PageItem::Text("演者：山田 太郎".into()),
            ],
        };
        let bytes = render_pdf(&d, &report, &PrintSettings::default());
        let source = String::from_utf8_lossy(&bytes);
        assert!(source.contains("/Subtype /Type0"));
        assert!(source.contains("/Subtype /CIDFontType2"));
        assert!(source.contains("/Encoding /Identity-H"));
        assert!(source.contains("/ToUnicode 7 0 R"));
        assert!(source.contains("/FontFile2 6 0 R"));
        assert!(bytes.windows(NOTO_SANS_JP.len()).any(|w| w == NOTO_SANS_JP));

        let extracted = extract_type0_text_for_test(&source);
        assert!(extracted.contains("ドリル図面"), "{extracted}");
        assert!(extracted.contains(phrase), "{extracted}");
        assert!(extracted.contains("演者：山田 太郎"), "{extracted}");
        assert!(!extracted.contains('?'));
    }

    /// Independent consumer for the serialized CMap and text operators.
    fn extract_type0_text_for_test(pdf: &str) -> String {
        let mut cmap = HashMap::new();
        for line in pdf.lines() {
            let line = line.trim();
            if line.len() >= 13
                && line.starts_with('<')
                && let Some((cid, unicode)) = line.split_once("> <")
            {
                let cid = cid.trim_start_matches('<');
                let unicode = unicode.trim_end_matches('>');
                if cid.len() == 4
                    && unicode.len().is_multiple_of(4)
                    && let (Ok(cid), Some(value)) =
                        (u16::from_str_radix(cid, 16), decode_utf16_hex(unicode))
                {
                    cmap.insert(cid, value);
                }
            }
        }
        let mut result = String::new();
        for line in pdf.lines().filter(|line| line.contains(" Tj ET")) {
            let Some(start) = line.rfind('<') else {
                continue;
            };
            let Some(end) = line[start + 1..].find('>') else {
                continue;
            };
            let encoded = &line[start + 1..start + 1 + end];
            for offset in (0..encoded.len()).step_by(4) {
                if let Ok(cid) = u16::from_str_radix(&encoded[offset..offset + 4], 16)
                    && let Some(value) = cmap.get(&cid)
                {
                    result.push_str(value);
                }
            }
            result.push('\n');
        }
        result
    }

    fn decode_utf16_hex(value: &str) -> Option<String> {
        let units = (0..value.len())
            .step_by(4)
            .map(|i| u16::from_str_radix(&value[i..i + 4], 16).ok())
            .collect::<Option<Vec<_>>>()?;
        char::decode_utf16(units)
            .map(|c| c.ok())
            .collect::<Option<String>>()
    }
}
