//! Self-contained, printable SVG / HTML export for browser "print to PDF".
//! Pure string building — no external crates, no runtime dependencies.
//!
//! Two families of output:
//! * [`field_svg`] / [`set_svg`] render one drill set as a labelled field diagram.
//! * [`coordinate_sheet_html`] / [`drill_book_html`] render printable coordinate
//!   sheets and per-performer "dot books".
//!
//! Field convention matches the rest of the crate: field `x` runs `0..grid.width`
//! left→right, depth `y` runs `0..grid.height` with `y = 0` the front sideline
//! (nearest the audience). In pixel space the front sideline is drawn at the
//! bottom, so increasing depth moves up the page.

use crate::Document;
use std::fmt::Write as _;

/// Escape the five XML/SVG-significant characters (`& < > " '`).
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// HTML text escaping — identical significant set to XML for our purposes.
fn html_escape(s: &str) -> String {
    xml_escape(s)
}

/// `[r, g, b]` → `#rrggbb`.
fn hex_color(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Trim a float to a compact fixed-precision string (`12.50 -> "12.5"`).
fn px(v: f32) -> String {
    format!("{v:.2}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// Yard-line number for a field x position: `min(x, width - x)` so it counts up
/// from each endzone to the center line.
fn yard_number(x: f32, width: f32) -> f32 {
    x.min(width - x)
}

/// Render a single set as a complete, standalone `<svg>...</svg>` string.
///
/// Draws the green field with border, vertical yard lines every
/// `grid.major_line_interval` (with yard-number labels), the horizontal hash
/// lines from `grid.hashes` (with labels), and every performer as a filled
/// circle in their own colour with their label. Field coordinates are scaled to
/// fit `width_px` × `height_px` while preserving aspect ratio and centered.
///
/// Returns a valid but essentially empty `<svg>` if `set_index` is out of range.
pub fn field_svg(doc: &Document, set_index: usize, width_px: f32, height_px: f32) -> String {
    let w = width_px.max(1.0);
    let h = height_px.max(1.0);

    let header = |extra: &str| {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" \
             viewBox=\"0 0 {} {}\">{}",
            px(w),
            px(h),
            px(w),
            px(h),
            extra
        )
    };

    let Some(set) = doc.sets.get(set_index) else {
        return format!("{}</svg>", header(""));
    };

    let grid = &doc.grid;
    let gw = grid.width.max(f32::EPSILON);
    let gh = grid.height.max(f32::EPSILON);

    // Fit the field into the pixel box, preserving aspect, leaving a margin for
    // labels around the edges.
    let margin = 34.0_f32.min(w * 0.08).min(h * 0.08);
    let inner_w = (w - margin * 2.0).max(1.0);
    let inner_h = (h - margin * 2.0).max(1.0);
    let scale = (inner_w / gw).min(inner_h / gh);
    let field_w = gw * scale;
    let field_h = gh * scale;
    let off_x = (w - field_w) / 2.0;
    let off_y = (h - field_h) / 2.0;

    // Field (x, depth y) -> pixel (px, py). Front sideline (y=0) at the bottom.
    let map = |fx: f32, fy: f32| -> (f32, f32) { (off_x + fx * scale, off_y + (gh - fy) * scale) };

    let mut body = String::with_capacity(2048);

    // Field background + border.
    let _ = write!(
        body,
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#2e7d32\" \
         stroke=\"#1b5e20\" stroke-width=\"2\"/>",
        px(off_x),
        px(off_y),
        px(field_w),
        px(field_h)
    );

    // Vertical yard lines + numbers.
    let interval = if grid.major_line_interval > 0.0 {
        grid.major_line_interval
    } else {
        5.0
    };
    let mut x = 0.0_f32;
    while x <= gw + 1e-3 {
        let (x1, y1) = map(x, 0.0);
        let (_x2, y2) = map(x, gh);
        let edge = x < 1e-3 || (x - gw).abs() < 1e-3;
        let stroke_w = if edge { 2.0 } else { 1.0 };
        let _ = write!(
            body,
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#ffffff\" \
             stroke-width=\"{}\" stroke-opacity=\"0.85\"/>",
            px(x1),
            px(y1),
            px(x1),
            px(y2),
            px(stroke_w)
        );
        let num = yard_number(x, gw);
        let label = format!("{}", num.round() as i64);
        let _ = write!(
            body,
            "<text x=\"{}\" y=\"{}\" fill=\"#ffffff\" font-family=\"sans-serif\" \
             font-size=\"12\" text-anchor=\"middle\">{}</text>",
            px(x1),
            px(y1 + 15.0),
            xml_escape(&label)
        );
        x += interval;
    }

    // Horizontal hash lines + labels.
    for hash in &grid.hashes {
        if hash.position < 0.0 || hash.position > gh {
            continue;
        }
        let (hx1, hy) = map(0.0, hash.position);
        let (hx2, _) = map(gw, hash.position);
        let sw = hash.weight.max(0.5);
        let _ = write!(
            body,
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#ffd54f\" \
             stroke-width=\"{}\" stroke-opacity=\"0.9\"/>",
            px(hx1),
            px(hy),
            px(hx2),
            px(hy),
            px(sw)
        );
        let _ = write!(
            body,
            "<text x=\"{}\" y=\"{}\" fill=\"#ffffff\" font-family=\"sans-serif\" \
             font-size=\"11\" text-anchor=\"start\">{}</text>",
            px(hx1 + 4.0),
            px(hy - 3.0),
            xml_escape(&hash.label)
        );
    }

    // Performers: one filled circle + label each, index-aligned with positions.
    for (i, performer) in doc.performers.iter().enumerate() {
        let p = set.positions.get(i).copied().unwrap_or_default();
        let (cx, cy) = map(p.x, p.y);
        let color = hex_color(performer.resolved_color(&doc.sections));
        let _ = write!(
            body,
            "<circle cx=\"{}\" cy=\"{}\" r=\"7\" fill=\"{}\" stroke=\"#000000\" \
             stroke-width=\"1\"/>",
            px(cx),
            px(cy),
            color
        );
        let _ = write!(
            body,
            "<text x=\"{}\" y=\"{}\" fill=\"#000000\" font-family=\"sans-serif\" \
             font-size=\"9\" text-anchor=\"middle\" dominant-baseline=\"central\">{}</text>",
            px(cx),
            px(cy),
            xml_escape(&performer.label)
        );
    }

    // Title (set name).
    let _ = write!(
        body,
        "<text x=\"{}\" y=\"{}\" fill=\"#000000\" font-family=\"sans-serif\" \
         font-size=\"16\" font-weight=\"bold\" text-anchor=\"middle\">{}</text>",
        px(w / 2.0),
        px(margin * 0.6 + 6.0),
        xml_escape(&set.name)
    );

    format!("{}</svg>", header(&body))
}

/// Convenience wrapper around [`field_svg`] with a default pixel size derived
/// from the grid aspect ratio (1000px wide).
pub fn set_svg(doc: &Document, set_index: usize) -> String {
    let grid = &doc.grid;
    let width_px = 1000.0_f32;
    let aspect = if grid.width > 0.0 {
        grid.height / grid.width
    } else {
        0.5
    };
    let height_px = (width_px * aspect).max(1.0);
    field_svg(doc, set_index, width_px, height_px)
}

/// Shared minimal print stylesheet for the HTML documents.
fn print_style() -> &'static str {
    "<style>\
     body{font-family:sans-serif;margin:24px;color:#111;}\
     h1{font-size:20px;}h2{font-size:16px;margin:0 0 8px;}\
     table{border-collapse:collapse;width:100%;margin-bottom:16px;}\
     th,td{border:1px solid #999;padding:4px 8px;text-align:left;font-size:12px;}\
     th{background:#eee;}\
     .page{page-break-before:always;}\
     @media print{body{margin:0;}th{background:#eee !important;\
     -webkit-print-color-adjust:exact;print-color-adjust:exact;}}\
     </style>"
}

/// Build a complete standalone HTML coordinate sheet: one table with a header
/// row and one data row per (performer, set) carrying the performer label, set
/// name, counts and the human-readable coordinate.
pub fn coordinate_sheet_html(doc: &Document) -> String {
    coordinate_sheet_html_localized(doc, crate::Locale::Ja)
}
pub fn coordinate_sheet_html_localized(doc: &Document, locale: crate::Locale) -> String {
    let mut out = String::with_capacity(4096);
    let (lang, title, columns) = match locale {
        crate::Locale::Ja => ("ja", "座標シート", ["演者", "セット", "カウント", "座標"]),
        crate::Locale::En => (
            "en",
            "Coordinate Sheet",
            ["Performer", "Set", "Counts", "Coordinate"],
        ),
    };
    let _ = write!(
        out,
        "<!DOCTYPE html><html lang=\"{lang}\"><head><meta charset=\"utf-8\">"
    );
    let _ = write!(
        out,
        "<title>{} — {}</title>",
        html_escape(&doc.title),
        title
    );
    out.push_str(print_style());
    out.push_str("</head><body>");
    let _ = write!(out, "<h1>{}</h1>", html_escape(&doc.title));
    let _ = write!(
        out,
        "<table><thead><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>",
        columns[0], columns[1], columns[2], columns[3]
    );
    for (i, performer) in doc.performers.iter().enumerate() {
        for set in &doc.sets {
            let coord = set
                .positions
                .get(i)
                .map(|&p| crate::coordinates::readable_localized(p, &doc.grid, locale))
                .unwrap_or_default();
            let _ = write!(
                out,
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&performer.label),
                html_escape(&set.name),
                set.counts,
                html_escape(&coord)
            );
        }
    }
    out.push_str("</tbody></table></body></html>");
    out
}

/// Build a standalone HTML "drill book": one section per performer with their
/// label as a heading and a small table of their coordinate at every set.
/// Each performer after the first begins on a fresh printed page.
pub fn drill_book_html(doc: &Document) -> String {
    drill_book_html_localized(doc, crate::Locale::Ja)
}
pub fn drill_book_html_localized(doc: &Document, locale: crate::Locale) -> String {
    let mut out = String::with_capacity(4096);
    let (lang, title, columns) = match locale {
        crate::Locale::Ja => ("ja", "ドットブック", ["セット", "カウント", "座標"]),
        crate::Locale::En => ("en", "Drill Book", ["Set", "Counts", "Coordinate"]),
    };
    let _ = write!(
        out,
        "<!DOCTYPE html><html lang=\"{lang}\"><head><meta charset=\"utf-8\">"
    );
    let _ = write!(
        out,
        "<title>{} — {}</title>",
        html_escape(&doc.title),
        title
    );
    out.push_str(print_style());
    out.push_str("</head><body>");
    for (i, performer) in doc.performers.iter().enumerate() {
        let cls = if i == 0 { "section" } else { "section page" };
        let _ = write!(out, "<div class=\"{}\">", cls);
        let _ = write!(out, "<h2>{}</h2>", html_escape(&performer.label));
        let _ = write!(
            out,
            "<table><thead><tr><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>",
            columns[0], columns[1], columns[2]
        );
        for set in &doc.sets {
            let coord = set
                .positions
                .get(i)
                .map(|&p| crate::coordinates::readable_localized(p, &doc.grid, locale))
                .unwrap_or_default();
            let _ = write!(
                out,
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                html_escape(&set.name),
                set.counts,
                html_escape(&coord)
            );
        }
        out.push_str("</tbody></table></div>");
    }
    out.push_str("</body></html>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_svg_is_well_formed() {
        let doc = Document::demo(3, 4);
        let svg = field_svg(&doc, 0, 1000.0, 600.0);
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn field_svg_draws_one_circle_per_performer() {
        let doc = Document::demo(3, 4);
        let svg = field_svg(&doc, 0, 1000.0, 600.0);
        assert_eq!(svg.matches("<circle").count(), doc.performers.len());
    }

    #[test]
    fn field_svg_out_of_range_is_valid_but_empty() {
        let doc = Document::demo(2, 2);
        let svg = field_svg(&doc, 99, 800.0, 400.0);
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert_eq!(svg.matches("<circle").count(), 0);
    }

    #[test]
    fn set_svg_uses_grid_aspect() {
        let doc = Document::demo(2, 2);
        let svg = set_svg(&doc, 0);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("width=\"1000\""));
        assert_eq!(svg.matches("<circle").count(), doc.performers.len());
    }

    #[test]
    fn coordinate_sheet_has_a_row_per_pair() {
        let doc = Document::demo(3, 3);
        let html = coordinate_sheet_html(&doc);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<table"));
        let rows = html.matches("<tr>").count();
        // Header row uses a plain <tr>, so data rows = total - 1.
        assert_eq!(rows - 1, doc.performers.len() * doc.sets.len());
    }

    #[test]
    fn drill_book_paginates_per_performer() {
        let doc = Document::demo(2, 2);
        let html = drill_book_html(&doc);
        assert!(html.starts_with("<!DOCTYPE html>"));
        // Every performer but the first gets a page break.
        assert_eq!(
            html.matches("section page").count(),
            doc.performers.len() - 1
        );
    }

    #[test]
    fn text_is_xml_and_html_escaped() {
        let mut doc = Document::demo(1, 1);
        doc.performers[0].label = "A & B <\"'>".into();

        let svg = field_svg(&doc, 0, 400.0, 400.0);
        assert!(svg.contains("A &amp; B"));
        assert!(!svg.contains("A & B"));

        let sheet = coordinate_sheet_html(&doc);
        assert!(sheet.contains("A &amp; B"));
        assert!(!sheet.contains("A & B"));

        let book = drill_book_html(&doc);
        assert!(book.contains("A &amp; B"));
    }

    #[test]
    fn english_legacy_html_localizes_all_built_in_labels() {
        let mut doc = Document::demo(1, 1);
        doc.title = "Show".into();
        for (i, set) in doc.sets.iter_mut().enumerate() {
            set.name = format!("Set {}", i + 1);
        }
        let html = coordinate_sheet_html_localized(&doc, crate::Locale::En);
        assert!(
            html.contains("lang=\"en\"")
                && html.contains("Coordinate Sheet")
                && html.contains("Performer")
        );
        assert!(
            !html.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)
                || ('\u{4e00}'..='\u{9fff}').contains(&c)),
            "{html}"
        );
    }

    #[test]
    fn html_coordinate_sheet_uses_persisted_notation() {
        let mut doc = Document::demo(1, 1);
        doc.grid.coordinate_notation = crate::coordinates::CoordinateNotation::dci();
        let html = coordinate_sheet_html_localized(&doc, crate::Locale::En);
        assert!(html.contains("8-to-5"));
        assert!(html.contains("front hash"));
    }
}
