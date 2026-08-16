//! Bounded XLSX worksheet extraction into the existing tabular import path.
use crate::{Delimiter, ImportError, ImportLimits, TabularPreview};
use std::collections::BTreeMap;
use std::io::{Cursor, Read};

const MAX_SHEETS: usize = 64;
const MAX_EXPANDED: usize = 64 << 20;
#[derive(Clone, Debug)]
pub struct XlsxSheet {
    pub name: String,
    pub preview: TabularPreview,
    pub csv: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct XlsxWorkbook {
    pub sheets: Vec<XlsxSheet>,
}

pub fn inspect_xlsx(bytes: &[u8], limits: &ImportLimits) -> Result<XlsxWorkbook, ImportError> {
    if bytes.len() > limits.max_bytes {
        return Err(ImportError::Limit {
            what: "xlsx bytes",
            limit: limits.max_bytes,
        });
    }
    let mut zip =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| malformed("invalid XLSX ZIP"))?;
    if zip.len() > 256 {
        return Err(ImportError::Limit {
            what: "xlsx entries",
            limit: 256,
        });
    }
    let mut expanded = 0usize;
    for i in 0..zip.len() {
        let f = zip
            .by_index(i)
            .map_err(|_| malformed("invalid ZIP entry"))?;
        let n = usize::try_from(f.size()).unwrap_or(usize::MAX);
        expanded = expanded.checked_add(n).ok_or(ImportError::Limit {
            what: "expanded XLSX",
            limit: MAX_EXPANDED,
        })?;
    }
    if expanded > MAX_EXPANDED {
        return Err(ImportError::Limit {
            what: "expanded XLSX",
            limit: MAX_EXPANDED,
        });
    }
    let workbook = read_entry(&mut zip, "xl/workbook.xml", 4 << 20)?;
    let rels = read_entry(&mut zip, "xl/_rels/workbook.xml.rels", 2 << 20)?;
    let shared = match read_entry(&mut zip, "xl/sharedStrings.xml", 32 << 20) {
        Ok(s) => parse_shared(&s, limits)?,
        Err(_) => Vec::new(),
    };
    let relations = parse_relations(&rels);
    let definitions = parse_sheets(&workbook);
    if definitions.len() > MAX_SHEETS {
        return Err(ImportError::Limit {
            what: "xlsx sheets",
            limit: MAX_SHEETS,
        });
    }
    let mut sheets = Vec::new();
    for (name, id) in definitions {
        let Some(target) = relations.get(&id) else {
            continue;
        };
        let path = if target.starts_with("xl/") {
            target.clone()
        } else {
            format!("xl/{}", target.trim_start_matches('/'))
        };
        let xml = read_entry(&mut zip, &path, MAX_EXPANDED)?;
        let rows = parse_worksheet(&xml, &shared, limits)?;
        if rows.is_empty() {
            continue;
        }
        let csv = encode_csv(&rows);
        let headers = rows[0].clone();
        let total_rows = rows.len() - 1;
        sheets.push(XlsxSheet {
            name,
            preview: TabularPreview {
                delimiter: Delimiter::Comma,
                headers,
                rows: rows
                    .into_iter()
                    .skip(1)
                    .take(limits.max_preview_rows)
                    .collect(),
                total_rows,
                truncated: total_rows > limits.max_preview_rows,
                replacement_characters: false,
            },
            csv,
        });
    }
    if sheets.is_empty() {
        return Err(ImportError::NoUsableRows);
    }
    Ok(XlsxWorkbook { sheets })
}
fn malformed(message: &'static str) -> ImportError {
    ImportError::Malformed { line: 0, message }
}
fn read_entry<R: std::io::Read + std::io::Seek>(
    z: &mut zip::ZipArchive<R>,
    name: &str,
    limit: usize,
) -> Result<String, ImportError> {
    let f = z
        .by_name(name)
        .map_err(|_| malformed("missing XLSX part"))?;
    if f.size() > limit as u64 {
        return Err(ImportError::Limit {
            what: "xlsx part",
            limit,
        });
    }
    let mut out = String::new();
    f.take(limit as u64 + 1)
        .read_to_string(&mut out)
        .map_err(|_| ImportError::InvalidEncoding)?;
    if out.len() > limit {
        return Err(ImportError::Limit {
            what: "xlsx part",
            limit,
        });
    }
    if out.contains("<!DOCTYPE") || out.contains("<!ENTITY") {
        return Err(malformed("DTD/entities are forbidden"));
    }
    Ok(out)
}
fn parse_relations(s: &str) -> BTreeMap<String, String> {
    tags(s, "Relationship")
        .into_iter()
        .filter_map(|t| {
            Some((
                attr(t, "Id")?.to_owned(),
                attr(t, "Target")?.replace("../", ""),
            ))
        })
        .collect()
}
fn parse_sheets(s: &str) -> Vec<(String, String)> {
    tags(s, "sheet")
        .into_iter()
        .filter_map(|t| Some((decode(attr(t, "name")?), attr(t, "r:id")?.to_owned())))
        .collect()
}
fn parse_shared(s: &str, l: &ImportLimits) -> Result<Vec<String>, ImportError> {
    let mut out = Vec::new();
    for si in blocks(s, "si") {
        if out.len() > l.max_rows {
            return Err(ImportError::Limit {
                what: "shared strings",
                limit: l.max_rows,
            });
        }
        let value = blocks(si, "t").map(strip_tags).collect::<String>();
        if value.len() > l.max_field_bytes {
            return Err(ImportError::Limit {
                what: "xlsx cell",
                limit: l.max_field_bytes,
            });
        }
        out.push(decode(&value));
    }
    Ok(out)
}
fn parse_worksheet(
    s: &str,
    shared: &[String],
    l: &ImportLimits,
) -> Result<Vec<Vec<String>>, ImportError> {
    let mut rows = Vec::new();
    for row in blocks(s, "row") {
        if rows.len() >= l.max_rows {
            return Err(ImportError::Limit {
                what: "xlsx rows",
                limit: l.max_rows,
            });
        }
        let mut cells = Vec::new();
        for cell in blocks(row, "c") {
            let open = cell.split_once('>').map(|x| x.0).unwrap_or(cell);
            let col = attr(open, "r")
                .and_then(column_index)
                .unwrap_or(cells.len());
            if col >= l.max_columns {
                return Err(ImportError::Limit {
                    what: "xlsx columns",
                    limit: l.max_columns,
                });
            }
            cells.resize(col + 1, String::new());
            let raw = blocks(cell, "v")
                .next()
                .map(strip_tags)
                .or_else(|| blocks(cell, "t").next().map(strip_tags))
                .unwrap_or_default();
            let value = if attr(open, "t") == Some("s") {
                raw.parse::<usize>()
                    .ok()
                    .and_then(|i| shared.get(i))
                    .cloned()
                    .unwrap_or_default()
            } else {
                decode(&raw)
            };
            if value.len() > l.max_field_bytes {
                return Err(ImportError::Limit {
                    what: "xlsx cell",
                    limit: l.max_field_bytes,
                });
            }
            cells[col] = value;
        }
        if !cells.is_empty() {
            rows.push(cells)
        }
    }
    Ok(rows)
}
fn column_index(reference: &str) -> Option<usize> {
    let mut v = 0usize;
    let mut any = false;
    for b in reference.bytes().take_while(u8::is_ascii_alphabetic) {
        any = true;
        v = v
            .checked_mul(26)?
            .checked_add(usize::from(b.to_ascii_uppercase() - b'A' + 1))?
    }
    any.then_some(v - 1)
}
fn tags<'a>(s: &'a str, name: &str) -> Vec<&'a str> {
    let needle = format!("<{name}");
    let mut result = Vec::new();
    let mut at = 0;
    while let Some(relative) = s[at..].find(&needle) {
        let i = at + relative;
        let Some(end) = s[i..].find('>').map(|v| i + v + 1) else {
            break;
        };
        let t = &s[i..end];
        if t.as_bytes()
            .get(needle.len())
            .is_some_and(|b| b.is_ascii_whitespace() || *b == b'/' || *b == b'>')
        {
            result.push(t);
        }
        at = end;
    }
    result
}
fn blocks<'a>(s: &'a str, name: &str) -> impl Iterator<Item = &'a str> {
    let start = format!("<{name}");
    let close = format!("</{name}>");
    let mut at = 0;
    std::iter::from_fn(move || {
        let i = s[at..].find(&start)? + at;
        let open = s[i..].find('>')? + i + 1;
        let end = s[open..].find(&close)? + open;
        at = end + close.len();
        Some(&s[i..at])
    })
}
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    for quote in ['"', '\''] {
        let n = format!("{name}={quote}");
        if let Some(i) = tag.find(&n) {
            let st = i + n.len();
            let en = tag[st..].find(quote)? + st;
            return Some(&tag[st..en]);
        }
    }
    None
}
fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in s.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out
}
fn decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}
fn encode_csv(rows: &[Vec<String>]) -> Vec<u8> {
    let mut out = String::new();
    for row in rows {
        for (i, v) in row.iter().enumerate() {
            if i > 0 {
                out.push(',')
            }
            if v.contains([',', '"', '\n', '\r']) {
                out.push('"');
                out.push_str(&v.replace('"', "\"\""));
                out.push('"')
            } else {
                out.push_str(v)
            }
        }
        out.push('\n')
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn book() -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        for (n, s) in [
            (
                "xl/workbook.xml",
                r#"<workbook><sheets><sheet name="Dots" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/sharedStrings.xml",
                r#"<sst><si><t>performer</t></si><si><t>set</t></si><si><t>x</t></si><si><t>y</t></si><si><t>A1</t></si></sst>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><sheetData><row><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c><c r="D1" t="s"><v>3</v></c></row><row><c r="A2" t="s"><v>4</v></c><c r="B2"><v>1</v></c><c r="C2"><v>10</v></c><c r="D2"><v>20</v></c></row></sheetData></worksheet>"#,
            ),
        ] {
            w.start_file(n, o).unwrap();
            w.write_all(s.as_bytes()).unwrap()
        }
        w.finish().unwrap().into_inner()
    }
    #[test]
    fn extracts_sheet_into_existing_mapping() {
        let x = inspect_xlsx(&book(), &Default::default()).unwrap();
        assert_eq!(x.sheets[0].name, "Dots");
        let m = crate::suggest_mapping(&x.sheets[0].preview.headers).unwrap();
        assert_eq!((m.performer, m.set, m.x, m.y), (0, 1, 2, 3));
        assert!(
            std::str::from_utf8(&x.sheets[0].csv)
                .unwrap()
                .contains("A1,1,10,20")
        );
    }
    #[test]
    fn rejects_non_zip_and_limits() {
        assert!(inspect_xlsx(b"bad", &Default::default()).is_err());
        let l = ImportLimits {
            max_bytes: 2,
            ..Default::default()
        };
        assert!(matches!(
            inspect_xlsx(&book(), &l),
            Err(ImportError::Limit { .. })
        ));
    }
}
