//! Self-contained, offline "practice viewer" HTML for individual performers.
//!
//! # Why this exists
//!
//! Marching band performers often want a phone-friendly way to see their own
//! dots, coordinates, and travel between sets without installing anything.
//! [`build_practice_viewer`] renders a *single* `.html` file with every asset
//! (CSS, JS, field diagrams as inline SVG) embedded inline, so it opens and
//! runs entirely offline in any modern mobile browser -- no server, no build
//! step, no external network request of any kind.
//!
//! # Design: one file for everyone, not one file per performer
//!
//! The document may contain up to [`drill_core::MAX_PERFORMERS`] performers
//! and [`drill_core::MAX_SETS`] sets. Naively emitting each performer's field
//! diagram as a fully separate inline SVG on their own page would duplicate
//! the (potentially large) per-set diagram once per performer, an O(N x S)
//! cost in diagram bytes on top of the O(N x S) cost that is unavoidable for
//! the textual coordinate/continuity data.
//!
//! Instead, this crate keeps the "everyone in one file" design the spec calls
//! for (a single link/attachment is easiest to distribute) but avoids the
//! diagram blow-up: each *set* gets exactly one `<template>` element holding
//! its field SVG (context dots for the whole roster, tagged with
//! `data-pid="<performer id>"`), so diagram bytes are O(S), not O(N x S).
//! Performer pages only hold a lightweight `<div class="field-mount">`
//! placeholder per set; a few lines of vanilla JS clone the matching
//! `<template>` into the mount the first time that performer's page is shown
//! and marks the matching dot as "self" vs. every other dot as dimmed
//! context. The textual per-(performer, set) content (coordinate readout,
//! continuity line, optional timing) is inherently O(N x S) -- there is no
//! way to share it -- but stays within a low single-digit-MB budget even at
//! [`drill_core::MAX_PERFORMERS`]-scale rosters because each line is short.
//!
//! # Bilingual by construction
//!
//! Every localized string is emitted *twice*, once as `<span lang="ja">` and
//! once as `<span lang="en">`, with a single CSS rule keyed off a `data-lang`
//! attribute on `<html>` hiding whichever language is not active. Switching
//! language is therefore a one-line JS attribute flip with no reformatting,
//! and both languages are always present in the output regardless of the
//! `locale` argument (which only selects the *initial* language and the
//! `lang="…"` document attribute).
//!
//! # Robustness
//!
//! This module never panics on untrusted input: unknown performer ids in
//! `performer_ids` are silently ignored, an empty document (no performers,
//! no sets) still produces well-formed HTML with an empty-state message, and
//! non-finite (`NaN`/`inf`) coordinates are detected and replaced with a
//! fallback message rather than being formatted (which would otherwise leak
//! literal `"NaN"` text into the page).
//!
//! Output is fully deterministic: no timestamps, random ids, or hash-map
//! iteration order are ever embedded, so the same [`Document`] always
//! produces byte-identical HTML.

use drill_core::{
    Document, DrillError, Locale, Performer, PerformerId, Point, SectionId, continuity, coordinates,
};
use drill_render::{FieldMap, Vec2};
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// Rendered width, in CSS pixels, of every field diagram. Diagrams scale to
/// their container via `width="100%"`, so this only sets the SVG's internal
/// coordinate space (and therefore aspect ratio together with the computed
/// height).
const FIELD_WIDTH_PX: f32 = 320.0;
/// Margin around the field inside each diagram's viewBox.
const FIELD_MARGIN_PX: f32 = 6.0;
/// Radius of a context (non-self) performer dot.
const DOT_RADIUS_PX: f32 = 3.4;

/// Builds a self-contained offline HTML practice viewer for one or more
/// performers.
///
/// If `performer_ids` is empty, every performer in `document` is included and
/// the viewer opens directly to the performer-selection screen. If
/// `performer_ids` is non-empty, only the matching performers (in the order
/// they appear in `document.performers`, deduplicated) are included; ids that
/// do not match any performer are silently ignored rather than causing an
/// error.
///
/// The returned string is a complete `<!doctype html>` document: opening it
/// directly as a local file works with no server and no network access.
pub fn build_practice_viewer(
    document: &Document,
    performer_ids: &[PerformerId],
    locale: Locale,
) -> Result<String, DrillError> {
    if document.performers.len() > drill_core::MAX_PERFORMERS {
        return Err(DrillError::LimitExceeded {
            field: "performers",
            limit: drill_core::MAX_PERFORMERS,
        });
    }
    if document.sets.len() > drill_core::MAX_SETS {
        return Err(DrillError::LimitExceeded {
            field: "sets",
            limit: drill_core::MAX_SETS,
        });
    }
    for (set_index, set) in document.sets.iter().enumerate() {
        if set.positions.len() != document.performers.len() {
            return Err(DrillError::SetSizeMismatch {
                set_index,
                expected: document.performers.len(),
                found: set.positions.len(),
            });
        }
    }
    let indices = resolve_performer_indices(document, performer_ids);
    let field_height = field_height_px(&document.grid);
    let capacity = estimate_capacity(document, &indices);
    let mut html = String::new();
    html.try_reserve(capacity)
        .map_err(|_| DrillError::LimitExceeded {
            field: "practice viewer bytes",
            limit: capacity,
        })?;

    write_head(&mut html, document, locale);
    html.push_str("<body>\n");
    write_topbar(&mut html);
    html.push_str("<main>\n");
    write_selector(&mut html, document, &indices);
    for &index in &indices {
        write_performer_page(&mut html, document, index);
    }
    html.push_str("</main>\n");
    write_field_templates(&mut html, document, field_height);
    write_script(&mut html);
    html.push_str("</body>\n</html>\n");
    Ok(html)
}

/// Resolves which performer indices to include, preserving roster order and
/// silently dropping unknown ids. An empty `performer_ids` means "everyone".
fn resolve_performer_indices(document: &Document, performer_ids: &[PerformerId]) -> Vec<usize> {
    if performer_ids.is_empty() {
        return (0..document.performers.len()).collect();
    }
    let wanted: BTreeSet<PerformerId> = performer_ids.iter().copied().collect();
    document
        .performers
        .iter()
        .enumerate()
        .filter(|(_, performer)| wanted.contains(&performer.id))
        .map(|(index, _)| index)
        .collect()
}

fn estimate_capacity(document: &Document, indices: &[usize]) -> usize {
    let per_set_card = 320usize;
    let sets = document.sets.len();
    let text_bytes = indices
        .len()
        .saturating_mul(sets)
        .saturating_mul(per_set_card);
    let diagram_bytes = sets
        .saturating_mul(document.performers.len())
        .saturating_mul(96);
    text_bytes
        .saturating_add(diagram_bytes)
        .saturating_add(8 * 1024)
}

fn field_height_px(grid: &drill_core::GridConfig) -> f32 {
    if grid.width.is_finite() && grid.width > 0.0 && grid.height.is_finite() && grid.height > 0.0 {
        (FIELD_WIDTH_PX * grid.height / grid.width).clamp(60.0, 640.0)
    } else {
        180.0
    }
}

fn point_is_finite(point: Point) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

/// Escapes text for safe placement in both HTML text nodes and
/// double-quoted attribute values.
fn esc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn svg_num(value: f32) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }
    let formatted = format!("{value:.3}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn hex_color(color: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

fn section_label(document: &Document, section: SectionId) -> &str {
    document
        .sections
        .iter()
        .find(|candidate| candidate.id == section)
        .map(|candidate| {
            if candidate.short.is_empty() {
                candidate.name.as_str()
            } else {
                candidate.short.as_str()
            }
        })
        .unwrap_or("")
}

fn write_head(html: &mut String, document: &Document, locale: Locale) {
    let lang = match locale {
        Locale::Ja => "ja",
        Locale::En => "en",
    };
    let _ = write!(
        html,
        "<!doctype html>\n<html lang=\"{lang}\" data-lang=\"{lang}\" data-field=\"on\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1, maximum-scale=1\">\n\
         <title>{} - Practice Viewer</title>\n\
         <style>{}</style>\n\
         </head>\n",
        esc(&document.title),
        CSS,
    );
}

fn write_topbar(html: &mut String) {
    html.push_str(
        "<header class=\"topbar\">\n\
         <h1><span lang=\"ja\">個人練習ビューア</span><span lang=\"en\">Practice Viewer</span></h1>\n\
         <div class=\"controls\">\n\
         <button type=\"button\" class=\"pill\" data-action=\"lang\" data-value=\"ja\">日本語</button>\n\
         <button type=\"button\" class=\"pill\" data-action=\"lang\" data-value=\"en\">English</button>\n\
         <button type=\"button\" class=\"pill\" data-action=\"field-toggle\">\
         <span lang=\"ja\">フィールド図 表示/非表示</span><span lang=\"en\">Toggle field chart</span>\
         </button>\n\
         </div>\n\
         </header>\n",
    );
}

fn write_selector(html: &mut String, document: &Document, indices: &[usize]) {
    html.push_str(
        "<section id=\"selector\">\n\
         <p class=\"hint\"><span lang=\"ja\">演者を選んでください</span>\
         <span lang=\"en\">Choose your performer</span></p>\n",
    );
    if indices.is_empty() {
        html.push_str(
            "<p class=\"empty\"><span lang=\"ja\">演者が登録されていません</span>\
             <span lang=\"en\">No performers are available</span></p>\n",
        );
    } else {
        html.push_str("<div class=\"grid\">\n");
        for &index in indices {
            let performer = &document.performers[index];
            let _ = writeln!(
                html,
                "<button type=\"button\" class=\"card\" data-pid=\"{}\">\
                 <span class=\"card-label\">{}</span>\
                 <span class=\"card-section\">{}</span>\
                 </button>",
                performer.id.get(),
                esc(&performer.label),
                esc(section_label(document, performer.section)),
            );
        }
        html.push_str("</div>\n");
    }
    html.push_str("</section>\n");
}

fn write_performer_page(html: &mut String, document: &Document, index: usize) {
    let performer: &Performer = &document.performers[index];
    let pid = performer.id.get();
    let _ = write!(
        html,
        "<section id=\"performer-{pid}\" class=\"performer-page\" data-pid=\"{pid}\" tabindex=\"-1\" hidden>\n\
         <button type=\"button\" class=\"back\">\
         <span lang=\"ja\">← 選択画面に戻る</span><span lang=\"en\">← Back to selection</span>\
         </button>\n\
         <h2>{}</h2>\n",
        esc(&performer.label),
    );
    if document.sets.is_empty() {
        html.push_str(
            "<p class=\"empty\"><span lang=\"ja\">セットがありません</span>\
             <span lang=\"en\">This drill has no sets</span></p>\n",
        );
    } else {
        html.push_str(
            "<nav class=\"set-nav\" aria-label=\"Set navigation\">\n\
             <button type=\"button\" data-action=\"prev-set\">\n\
             <span lang=\"ja\">← 前のセット</span><span lang=\"en\">← Previous</span></button>\n\
             <span class=\"set-progress\" aria-live=\"polite\"></span>\n\
             <button type=\"button\" data-action=\"next-set\">\n\
             <span lang=\"ja\">次のセット →</span><span lang=\"en\">Next →</span></button>\n\
             </nav>\n",
        );
        let segments = continuity::performer_continuity(document, index);
        for (set_index, set) in document.sets.iter().enumerate() {
            write_set_card(html, document, set_index, set, index, &segments);
        }
    }
    html.push_str("</section>\n");
}

fn write_set_card(
    html: &mut String,
    document: &Document,
    set_index: usize,
    set: &drill_core::Set,
    performer_index: usize,
    segments: &[continuity::ContinuitySegment],
) {
    let name = esc(&set.name);
    let _ = write!(
        html,
        "<article class=\"set-card\" data-set-index=\"{set_index}\">\n\
         <h3><span lang=\"ja\">{name} ({}カウント)</span><span lang=\"en\">{name} ({} counts)</span></h3>\n",
        set.counts, set.counts,
    );

    let here = set
        .positions
        .get(performer_index)
        .copied()
        .filter(|&point| point_is_finite(point));
    match here {
        Some(point) => {
            let ja = coordinates::readable_localized(point, &document.grid, Locale::Ja);
            let en = coordinates::readable_localized(point, &document.grid, Locale::En);
            let _ = writeln!(
                html,
                "<p class=\"coord\"><span lang=\"ja\">{}</span><span lang=\"en\">{}</span></p>",
                esc(&ja),
                esc(&en),
            );
        }
        None => html.push_str(
            "<p class=\"coord\"><span lang=\"ja\">座標データが不正です</span>\
             <span lang=\"en\">Invalid coordinate data</span></p>\n",
        ),
    }

    if let Some(segment) = segments.get(set_index) {
        let next_here = document
            .sets
            .get(set_index + 1)
            .and_then(|next| next.positions.get(performer_index))
            .copied()
            .is_some_and(point_is_finite);
        if here.is_some() && next_here {
            let ja = continuity::format_segment(segment, Locale::Ja);
            let en = continuity::format_segment(segment, Locale::En);
            let _ = writeln!(
                html,
                "<p class=\"move\"><span lang=\"ja\">{}</span><span lang=\"en\">{}</span></p>",
                esc(&ja),
                esc(&en),
            );
        } else {
            html.push_str(
                "<p class=\"move\"><span lang=\"ja\">移動データが不正です</span>\
                 <span lang=\"en\">Invalid movement data</span></p>\n",
            );
        }
    }

    if document.audio.is_some() {
        let global_count = document.global_count(set_index, 0.0);
        let seconds = document.tempo.seconds_at(global_count);
        if seconds.is_finite() {
            let (ja, en) = format_time_label(seconds);
            let _ = writeln!(
                html,
                "<p class=\"time\"><span lang=\"ja\">{}</span><span lang=\"en\">{}</span></p>",
                esc(&ja),
                esc(&en),
            );
        }
    }

    let _ = write!(
        html,
        "<div class=\"field-container\"><div class=\"field-mount\" data-set=\"{set_index}\"></div></div>\n\
         </article>\n",
    );
}

fn format_time_label(seconds: f32) -> (String, String) {
    let total = seconds.max(0.0).round() as u64;
    let minutes = total / 60;
    let secs = total % 60;
    let clock = format!("{minutes}:{secs:02}");
    (
        format!("開始から {clock} 後"),
        format!("{clock} from start"),
    )
}

fn write_field_templates(html: &mut String, document: &Document, field_height: f32) {
    for (set_index, set) in document.sets.iter().enumerate() {
        let svg = build_field_template_svg(document, &set.positions, FIELD_WIDTH_PX, field_height);
        let _ = writeln!(
            html,
            "<template id=\"field-tpl-{set_index}\">{svg}</template>"
        );
    }
}

/// Builds one set's field diagram as inline SVG, reusing
/// [`drill_render::FieldMap`] for the field<->screen transform so this
/// diagram can never drift from the transform used by the live 2D editor or
/// the other exporters.
fn build_field_template_svg(
    document: &Document,
    positions: &[Point],
    width: f32,
    height: f32,
) -> String {
    let grid = &document.grid;
    let map = FieldMap::new(
        grid.width,
        grid.height,
        Vec2 {
            x: width,
            y: height,
        },
        FIELD_MARGIN_PX,
    );
    let mut svg = String::with_capacity(positions.len() * 72 + 256);
    let _ = write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"100%\">",
        svg_num(width),
        svg_num(height),
    );

    let corner_a = map.map(Point { x: 0.0, y: 0.0 });
    let corner_b = map.map(Point {
        x: grid.width,
        y: grid.height,
    });
    let _ = write!(
        svg,
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" class=\"turf\"/>",
        svg_num(corner_a.x.min(corner_b.x)),
        svg_num(corner_a.y.min(corner_b.y)),
        svg_num((corner_b.x - corner_a.x).abs()),
        svg_num((corner_b.y - corner_a.y).abs()),
    );

    for hash in &grid.hashes {
        if hash.position.is_finite() {
            let y = hash.position.clamp(0.0, grid.height);
            let a = map.map(Point { x: 0.0, y });
            let b = map.map(Point { x: grid.width, y });
            let _ = write!(
                svg,
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" class=\"hash\"/>",
                svg_num(a.x),
                svg_num(a.y),
                svg_num(b.x),
                svg_num(b.y),
            );
        }
    }
    for y in [0.0, grid.height] {
        let a = map.map(Point { x: 0.0, y });
        let b = map.map(Point { x: grid.width, y });
        let _ = write!(
            svg,
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" class=\"sideline\"/>",
            svg_num(a.x),
            svg_num(a.y),
            svg_num(b.x),
            svg_num(b.y),
        );
    }

    for (performer, &point) in document.performers.iter().zip(positions) {
        if point_is_finite(point) {
            let screen = map.map(point);
            let color = performer.resolved_color(&document.sections);
            let _ = write!(
                svg,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" data-pid=\"{}\" style=\"fill:{}\"/>",
                svg_num(screen.x),
                svg_num(screen.y),
                svg_num(DOT_RADIUS_PX),
                performer.id.get(),
                hex_color(color),
            );
        }
    }
    svg.push_str("</svg>");
    svg
}

const CSS: &str = r#"
:root{color-scheme:light dark;--bg:#f4f6f5;--panel:#ffffff;--text:#14181a;--muted:#5b6a66;--accent:#1f7a4d;--border:#d7ddda;--turf:#e7f1ea;--sideline:#233;--hash:#8fae9c;--self:#d94f2b;--other:#98a5a0;}
@media (prefers-color-scheme:dark){:root{--bg:#0f1412;--panel:#182019;--text:#eef2ef;--muted:#9fb0aa;--accent:#57c98a;--border:#2b3630;--turf:#132a1d;--sideline:#dfe8e3;--hash:#4f6e5e;--self:#ff7a4d;--other:#4d5a55;}}
*{box-sizing:border-box;}
body{margin:0;background:var(--bg);color:var(--text);font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Hiragino Sans,Noto Sans JP,Roboto,sans-serif;-webkit-tap-highlight-color:transparent;}
html[data-lang="ja"] [lang="en"]{display:none;}
html[data-lang="en"] [lang="ja"]{display:none;}
html[data-field="off"] .field-container{display:none;}
.topbar{position:sticky;top:0;z-index:5;display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:.5rem;padding:.75rem 1rem;background:var(--panel);border-bottom:1px solid var(--border);}
.topbar h1{font-size:1.05rem;margin:0;}
.controls{display:flex;flex-wrap:wrap;gap:.4rem;}
.pill{border:1px solid var(--border);background:transparent;color:var(--text);border-radius:999px;padding:.5rem .9rem;font-size:.85rem;min-height:2.25rem;}
main{padding:.75rem 1rem 3rem;max-width:640px;margin:0 auto;}
.hint{color:var(--muted);}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(120px,1fr));gap:.6rem;}
.card{display:flex;flex-direction:column;align-items:flex-start;gap:.15rem;padding:1rem .9rem;min-height:64px;border-radius:14px;border:1px solid var(--border);background:var(--panel);color:var(--text);font-size:1rem;text-align:left;}
.card-label{font-weight:700;font-size:1.15rem;}
.card-section{color:var(--muted);font-size:.85rem;}
.back{margin:.25rem 0 1rem;border:none;background:transparent;color:var(--accent);font-size:1rem;padding:.5rem 0;}
.set-nav{position:sticky;top:3.65rem;z-index:4;display:flex;align-items:center;justify-content:space-between;gap:.35rem;margin:0 0 .75rem;padding:.45rem;border:1px solid var(--border);border-radius:12px;background:color-mix(in srgb,var(--panel) 94%,transparent);backdrop-filter:blur(14px);}
.set-nav button{border:0;background:transparent;color:var(--accent);font-size:.92rem;padding:.45rem .35rem;min-height:2.25rem;}
.set-nav button:disabled{color:var(--muted);opacity:.6;}
.set-progress{font-size:.85rem;color:var(--muted);font-variant-numeric:tabular-nums;white-space:nowrap;}
.set-card{border:1px solid var(--border);background:var(--panel);border-radius:14px;padding:1rem;margin-bottom:.85rem;}
.set-card h3{margin:0 0 .5rem;font-size:1.05rem;}
.set-card p{margin:.25rem 0;}
.set-card .time{color:var(--muted);font-size:.9rem;}
.field-container{margin-top:.6rem;border-radius:10px;overflow:hidden;background:var(--turf);}
.field-mount svg{display:block;width:100%;height:auto;}
.field-mount .turf{fill:var(--turf);}
.field-mount .sideline{stroke:var(--sideline);stroke-width:1.5;}
.field-mount .hash{stroke:var(--hash);stroke-width:1;}
.field-mount circle.other{fill:var(--other) !important;opacity:.55;}
.field-mount circle.self{opacity:1;r:7;stroke:var(--text);stroke-width:1.2;}
.empty{color:var(--muted);padding:1rem 0;}
"#;

fn write_script(html: &mut String) {
    html.push_str(
        r#"<script>
(function(){
"use strict";
var root=document.documentElement;
document.querySelectorAll('[data-action="lang"]').forEach(function(btn){
  btn.addEventListener('click',function(){
    root.dataset.lang=btn.dataset.value;
    root.lang=btn.dataset.value;
  });
});
document.querySelectorAll('[data-action="field-toggle"]').forEach(function(btn){
  btn.addEventListener('click',function(){
    root.dataset.field = root.dataset.field === 'on' ? 'off' : 'on';
  });
});
var selector=document.getElementById('selector');
var pages=Array.prototype.slice.call(document.querySelectorAll('.performer-page'));
function showSelector(){
  if(selector){selector.hidden=false;}
  pages.forEach(function(p){p.hidden=true;});
}
function mountFields(page){
  if(page.dataset.mounted==='1'){return;}
  page.dataset.mounted='1';
  var pid=page.dataset.pid;
  page.querySelectorAll('.field-mount').forEach(function(mount){
    var tpl=document.getElementById('field-tpl-'+mount.dataset.set);
    if(!tpl){return;}
    var clone=tpl.content.cloneNode(true);
    clone.querySelectorAll('circle[data-pid]').forEach(function(circle){
      if(circle.getAttribute('data-pid')===pid){circle.classList.add('self');}
      else{circle.classList.add('other');}
    });
    mount.appendChild(clone);
  });
}
function showSet(page, requested){
  var cards=Array.prototype.slice.call(page.querySelectorAll('.set-card'));
  if(!cards.length){return;}
  var index=Math.max(0,Math.min(cards.length-1,requested));
  cards.forEach(function(card,i){card.hidden=i!==index;});
  page.dataset.setIndex=String(index);
  var progress=page.querySelector('.set-progress');
  if(progress){progress.textContent=(index+1)+' / '+cards.length;}
  var prev=page.querySelector('[data-action="prev-set"]');
  var next=page.querySelector('[data-action="next-set"]');
  if(prev){prev.disabled=index===0;}
  if(next){next.disabled=index===cards.length-1;}
}
function wireSetNavigation(page){
  if(page.dataset.navWired==='1'){return;}
  page.dataset.navWired='1';
  var prev=page.querySelector('[data-action="prev-set"]');
  var next=page.querySelector('[data-action="next-set"]');
  if(prev){prev.addEventListener('click',function(){showSet(page,Number(page.dataset.setIndex||0)-1);});}
  if(next){next.addEventListener('click',function(){showSet(page,Number(page.dataset.setIndex||0)+1);});}
  page.addEventListener('keydown',function(event){
    if(event.altKey||event.ctrlKey||event.metaKey||event.target.matches('button,input,select,textarea')){return;}
    if(event.key==='ArrowLeft'){showSet(page,Number(page.dataset.setIndex||0)-1);event.preventDefault();}
    if(event.key==='ArrowRight'){showSet(page,Number(page.dataset.setIndex||0)+1);event.preventDefault();}
  });
}
function showPerformer(pid){
  if(selector){selector.hidden=true;}
  pages.forEach(function(p){
    var match=p.dataset.pid===pid;
    p.hidden=!match;
    if(match){mountFields(p);wireSetNavigation(p);showSet(p,Number(p.dataset.setIndex||0));p.focus();}
  });
}
document.querySelectorAll('.card').forEach(function(card){
  card.addEventListener('click',function(){showPerformer(card.dataset.pid);});
});
document.querySelectorAll('.back').forEach(function(btn){
  btn.addEventListener('click',showSelector);
});
showSelector();
})();
</script>
"#,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use drill_core::SetId;

    fn balanced(html: &str, open: &str, close: &str) -> bool {
        html.matches(open).count() == html.matches(close).count()
    }

    #[test]
    fn output_is_well_formed_enough() {
        let doc = Document::demo(3, 3);
        let html = build_practice_viewer(&doc, &[], Locale::Ja).unwrap();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.trim_end().ends_with("</html>"));
        assert!(balanced(&html, "<section", "</section>"));
        assert!(balanced(&html, "<article", "</article>"));
        assert!(balanced(&html, "<button", "</button>"));
        assert!(balanced(&html, "<div ", "</div>"));
        assert!(balanced(&html, "<span ", "</span>"));
        assert!(balanced(&html, "<p ", "</p>"));
        assert!(balanced(&html, "<h1", "</h1>"));
        assert!(balanced(&html, "<h2", "</h2>"));
        assert!(balanced(&html, "<h3", "</h3>"));
        assert!(balanced(&html, "<style", "</style>"));
        assert!(balanced(&html, "<script", "</script>"));
    }

    #[test]
    fn rejects_unvalidated_position_mismatch_before_rendering() {
        let mut doc = Document::demo(2, 2);
        doc.sets[0].positions.pop();
        assert!(matches!(
            build_practice_viewer(&doc, &[], Locale::Ja),
            Err(DrillError::SetSizeMismatch { set_index: 0, .. })
        ));
    }

    #[test]
    fn generation_is_deterministic() {
        let doc = Document::demo(4, 5);
        let a = build_practice_viewer(&doc, &[], Locale::Ja).unwrap();
        let b = build_practice_viewer(&doc, &[], Locale::Ja).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn empty_ids_include_everyone_and_open_the_selector() {
        let doc = Document::demo(2, 2);
        let html = build_practice_viewer(&doc, &[], Locale::Ja).unwrap();
        for performer in &doc.performers {
            assert!(html.contains(&format!("data-pid=\"{}\"", performer.id.get())));
        }
        assert!(html.contains("id=\"selector\""));
    }

    #[test]
    fn explicit_ids_filter_to_only_those_performers() {
        let doc = Document::demo(2, 3);
        let wanted = [doc.performers[0].id, doc.performers[2].id];
        let html = build_practice_viewer(&doc, &wanted, Locale::Ja).unwrap();
        assert!(html.contains(&format!("id=\"performer-{}\"", wanted[0].get())));
        assert!(html.contains(&format!("id=\"performer-{}\"", wanted[1].get())));
        assert!(!html.contains(&format!("id=\"performer-{}\"", doc.performers[1].id.get())));
    }

    #[test]
    fn unknown_ids_are_ignored_without_panicking() {
        let doc = Document::demo(1, 2);
        let bogus = PerformerId::new(999_999).unwrap();
        let html = build_practice_viewer(&doc, &[bogus, doc.performers[0].id], Locale::En).unwrap();
        assert!(html.contains(&format!("id=\"performer-{}\"", doc.performers[0].id.get())));
        assert!(!html.contains("performer-999999"));
    }

    #[test]
    fn both_locales_are_always_embedded() {
        let doc = Document::demo(1, 1);
        let point = doc.sets[0].positions[0];
        let ja = coordinates::readable_localized(point, &doc.grid, Locale::Ja);
        let en = coordinates::readable_localized(point, &doc.grid, Locale::En);
        for locale in [Locale::Ja, Locale::En] {
            let html = build_practice_viewer(&doc, &[], locale).unwrap();
            assert!(
                html.contains(&esc(&ja)),
                "missing Japanese text for {locale:?}"
            );
            assert!(
                html.contains(&esc(&en)),
                "missing English text for {locale:?}"
            );
            assert!(html.contains("個人練習ビューア"));
            assert!(html.contains("Practice Viewer"));
        }
    }

    #[test]
    fn performer_pages_include_single_set_practice_navigation() {
        let doc = Document::demo(1, 2);
        let html = build_practice_viewer(&doc, &[], Locale::En).unwrap();
        assert!(html.contains("class=\"set-nav\""));
        assert!(html.contains("data-action=\"prev-set\""));
        assert!(html.contains("data-action=\"next-set\""));
        assert!(html.contains("function showSet(page, requested)"));
        for set_index in 0..doc.sets.len() {
            assert!(html.contains(&format!("data-set-index=\"{set_index}\"")));
        }
    }

    #[test]
    fn empty_document_does_not_panic() {
        let mut doc = Document::demo(1, 1);
        doc.sets.clear();
        doc.performers.clear();
        let html = build_practice_viewer(&doc, &[], Locale::Ja).unwrap();
        assert!(html.contains("<!doctype html>"));
    }

    #[test]
    fn nan_positions_do_not_panic_or_leak_into_output() {
        let mut doc = Document::demo(1, 1);
        doc.sets[0].positions[0].x = f32::NAN;
        doc.sets[1].positions[0].y = f32::INFINITY;
        let html = build_practice_viewer(&doc, &[], Locale::Ja).unwrap();
        assert!(!html.contains("NaN"));
        assert!(!html.contains("inf"));
    }

    #[test]
    fn field_diagram_reuses_field_map_transform() {
        let doc = Document::demo(1, 1);
        let expected = FieldMap::new(
            doc.grid.width,
            doc.grid.height,
            Vec2 {
                x: FIELD_WIDTH_PX,
                y: field_height_px(&doc.grid),
            },
            FIELD_MARGIN_PX,
        )
        .map(doc.sets[0].positions[0]);
        let svg = build_field_template_svg(
            &doc,
            &doc.sets[0].positions,
            FIELD_WIDTH_PX,
            field_height_px(&doc.grid),
        );
        assert!(svg.contains(&format!("cx=\"{}\"", svg_num(expected.x))));
        assert!(svg.contains(&format!("cy=\"{}\"", svg_num(expected.y))));
    }

    #[test]
    fn large_roster_generates_quickly_and_stays_reasonably_sized() {
        let mut doc = Document::demo(25, 40); // 1,000 performers.
        let extra_template = doc.sets[1].clone();
        for i in 0..6u32 {
            let mut extra = extra_template.clone();
            extra.id = SetId::new(1000 + i).unwrap();
            extra.name = format!("Extra {}", i + 1);
            doc.sets.push(extra);
        }
        let start = std::time::Instant::now();
        let html = build_practice_viewer(&doc, &[], Locale::Ja).unwrap();
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(1500),
            "generation took {elapsed:?}, expected well under 1.5s"
        );
        assert!(
            html.len() < 8 * 1024 * 1024,
            "output was {} bytes, expected a few MB at most",
            html.len()
        );
    }
}
