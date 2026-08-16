//! Deterministic physical-page layout for printed reports.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageSize {
    A4,
    Letter,
    Tabloid,
    Custom { width_mm: f32, height_mm: f32 },
}

impl PageSize {
    pub fn dimensions_mm(self) -> (f32, f32) {
        match self {
            Self::A4 => (210.0, 297.0),
            Self::Letter => (215.9, 279.4),
            Self::Tabloid => (279.4, 431.8),
            Self::Custom {
                width_mm,
                height_mm,
            } => (width_mm.max(1.0), height_mm.max(1.0)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margins {
    pub top_mm: f32,
    pub right_mm: f32,
    pub bottom_mm: f32,
    pub left_mm: f32,
}

impl Margins {
    pub const fn uniform(mm: f32) -> Self {
        Self {
            top_mm: mm,
            right_mm: mm,
            bottom_mm: mm,
            left_mm: mm,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrintSettings {
    pub page_size: PageSize,
    pub orientation: Orientation,
    pub margins: Margins,
    pub title: String,
}

impl Default for PrintSettings {
    fn default() -> Self {
        Self {
            page_size: PageSize::A4,
            orientation: Orientation::Portrait,
            margins: Margins::uniform(12.0),
            title: String::new(),
        }
    }
}

impl PrintSettings {
    pub fn dimensions_mm(&self) -> (f32, f32) {
        let (w, h) = self.page_size.dimensions_mm();
        match self.orientation {
            Orientation::Portrait => (w, h),
            Orientation::Landscape => (h, w),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PageItem {
    Heading(String),
    Text(String),
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    FieldDiagram {
        set_index: usize,
    },
    PageBreak,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReportDocument {
    pub title: String,
    pub items: Vec<PageItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LaidOutPage {
    pub number: usize,
    pub items: Vec<PageItem>,
}

/// Stable pagination. Table headers are repeated and rows are never split.
pub fn paginate(report: &ReportDocument, settings: &PrintSettings) -> Vec<LaidOutPage> {
    let (_, height) = settings.dimensions_mm();
    let usable = (height - settings.margins.top_mm - settings.margins.bottom_mm - 14.0).max(20.0);
    let mut pages = vec![LaidOutPage {
        number: 1,
        items: Vec::new(),
    }];
    let mut used = 0.0_f32;
    let new_page = |pages: &mut Vec<LaidOutPage>, used: &mut f32| {
        pages.push(LaidOutPage {
            number: pages.len() + 1,
            items: Vec::new(),
        });
        *used = 0.0;
    };
    for item in &report.items {
        match item {
            PageItem::PageBreak => {
                if !pages.last().unwrap().items.is_empty() {
                    new_page(&mut pages, &mut used);
                }
            }
            PageItem::Table { columns, rows } => {
                let header_h = 7.0;
                let row_h = 6.0;
                let mut offset = 0;
                while offset < rows.len() {
                    if used + header_h + row_h > usable && !pages.last().unwrap().items.is_empty() {
                        new_page(&mut pages, &mut used);
                    }
                    let capacity = ((usable - used - header_h) / row_h).floor().max(1.0) as usize;
                    let end = (offset + capacity).min(rows.len());
                    pages.last_mut().unwrap().items.push(PageItem::Table {
                        columns: columns.clone(),
                        rows: rows[offset..end].to_vec(),
                    });
                    used += header_h + (end - offset) as f32 * row_h;
                    offset = end;
                    if offset < rows.len() {
                        new_page(&mut pages, &mut used);
                    }
                }
            }
            _ => {
                let h = match item {
                    PageItem::Heading(_) => 12.0,
                    PageItem::Text(_) => 7.0,
                    PageItem::FieldDiagram { .. } => usable.min(150.0),
                    _ => 0.0,
                };
                if used + h > usable && !pages.last().unwrap().items.is_empty() {
                    new_page(&mut pages, &mut used);
                }
                pages.last_mut().unwrap().items.push(item.clone());
                used += h;
            }
        }
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_rows_are_paginated_and_headers_repeated() {
        let report = ReportDocument {
            title: "x".into(),
            items: vec![PageItem::Table {
                columns: vec!["A".into()],
                rows: (0..100).map(|n| vec![n.to_string()]).collect(),
            }],
        };
        let pages = paginate(&report, &PrintSettings::default());
        assert!(pages.len() > 1);
        assert_eq!(
            pages
                .iter()
                .flat_map(|p| p.items.iter())
                .map(|i| match i {
                    PageItem::Table { rows, .. } => rows.len(),
                    _ => 0,
                })
                .sum::<usize>(),
            100
        );
    }
}
