//! Icon row for formation stamps. The geometry lives in `drill-core`.

use super::{BLUE, INK, PAPER, PhoneApp, local_vec, screen_pos, word};
use crate::touch::{FieldView, hit_radius};
use drill_core::stamps::{Stamp, StampGrab, StampKind, placed_positions};
use drill_core::{Document, Point, SetId};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2 as EVec2};

const WHITE: Color32 = Color32::WHITE;

pub(super) struct StampSession {
    pub(super) basis: Vec<Point>,
    pub(super) selected: Vec<usize>,
    pub(super) kind: StampKind,
    pub(super) letter: String,
    pub(super) stamp: Option<Stamp>,
    drag: Option<Drag>,
    last: Option<Point>,
}

enum Drag {
    Create { origin: Point },
    Body { start: Stamp, origin: Point },
    Resize { start: Stamp },
    Rotate { start: Stamp },
}

impl PhoneApp {
    pub(super) fn stamp_open(&self) -> bool {
        self.stamp_session.is_some()
    }

    pub(super) fn stamp_strip_height(&self, _compact: bool) -> f32 {
        match &self.stamp_session {
            Some(session) if session.kind == StampKind::Letter => 112.0,
            Some(_) => 64.0,
            None => 0.0,
        }
    }

    pub(super) fn toggle_stamp(&mut self) {
        if self.stamp_session.is_some() {
            self.close_stamp();
        } else {
            self.open_stamp();
        }
    }

    pub(super) fn open_stamp(&mut self) {
        if self.view_only {
            self.status = super::viewer_message(self.ja, self.editor_limit);
            return;
        }
        self.playing = false;
        let basis = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.positions.clone())
            .unwrap_or_default();
        let selected: Vec<usize> = (0..basis.len()).collect();
        self.stamp_session = Some(StampSession {
            basis,
            selected,
            kind: StampKind::Line,
            letter: "A".into(),
            stamp: None,
            drag: None,
            last: None,
        });
    }

    pub(super) fn close_stamp(&mut self) {
        self.publish_stamp();
        self.stamp_session = None;
    }

    pub(super) fn undo_stamp(&mut self) {
        let Some(session) = &self.stamp_session else {
            return;
        };
        let basis = session.basis.clone();
        let selected = session.selected.clone();
        if let Some(set) = self.document.sets.get_mut(self.current_set) {
            set.positions = basis.clone();
        }
        if let Some(session) = &mut self.stamp_session {
            session.stamp = None;
            session.drag = None;
        }
        self.publish_indexes(&selected, &basis);
    }

    pub(super) fn rebind_stamp(&mut self) {
        if self.stamp_session.is_none() {
            return;
        }
        self.stamp_session = None;
        self.open_stamp();
    }

    pub(super) fn install_stamp_preview(&mut self) {
        self.load_document(school_show());
        self.open_stamp();
        let stamp = Stamp {
            kind: StampKind::Circle,
            center: Point { x: 50.0, y: 26.7 },
            width: 36.0,
            height: 36.0,
            rotation: 0.0,
            text: "A".into(),
        };
        if let Some(session) = &mut self.stamp_session {
            session.kind = StampKind::Circle;
            session.stamp = Some(stamp);
        }
        self.apply_stamp_live();
        let positions = self
            .document
            .sets
            .first()
            .map(|set| set.positions.clone())
            .unwrap_or_default();
        self.view = FieldView::frame_cast(
            self.document.grid.width,
            self.document.grid.height,
            &positions,
        );
    }

    pub(super) fn stamp_pointer(
        &mut self,
        response: &egui::Response,
        rect: Rect,
        map: &drill_render::FieldMap,
        compact: bool,
    ) {
        let Some(pos) = response.interact_pointer_pos() else {
            if response.drag_stopped() {
                self.finish_stamp_drag(None, map, compact);
            }
            return;
        };
        let at = map.unmap(local_vec(rect, pos));
        if response.drag_started() {
            self.begin_stamp_drag(at, map, compact);
        }
        if response.dragged() {
            self.update_stamp_drag(at);
        }
        if response.drag_stopped() {
            self.finish_stamp_drag(Some(at), map, compact);
        }
    }

    pub(super) fn stamp_strip(&mut self, ui: &mut egui::Ui, rect: Rect, compact: bool) {
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, PAPER);
        painter.hline(
            rect.x_range(),
            rect.top(),
            Stroke::new(1.0, Color32::from_rgb(210, 214, 220)),
        );
        let ja = self.ja;
        let kind = self
            .stamp_session
            .as_ref()
            .map(|session| session.kind)
            .unwrap_or(StampKind::Line);
        let mut letter = self
            .stamp_session
            .as_ref()
            .map(|session| session.letter.clone())
            .unwrap_or_else(|| "A".into());
        let side = if compact { 40.0 } else { 44.0 };
        let top = Rect::from_min_size(
            rect.min + EVec2::new(8.0, 8.0),
            EVec2::new((rect.width() - 16.0).max(1.0), side),
        );
        let mut picked: Option<StampKind> = None;
        let mut undo = false;
        let mut confirm = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(top), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for kind_icon in StampKind::ALL {
                    if kind_button(ui, kind_icon, kind == kind_icon, side).clicked() {
                        picked = Some(kind_icon);
                    }
                }
                if kind_button_mark(ui, side, false, |painter, icon| {
                    paint_undo(painter, icon);
                })
                .clicked()
                {
                    undo = true;
                }
                if kind_button_mark(ui, side, true, |painter, icon| {
                    paint_check(painter, icon);
                })
                .clicked()
                {
                    confirm = true;
                }
            });
        });
        if kind == StampKind::Letter {
            let row = Rect::from_min_size(
                Pos2::new(rect.min.x + 8.0, rect.min.y + side + 16.0),
                EVec2::new((rect.width() - 16.0).max(1.0), 36.0),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(word(ja, "文字", "Text"))
                            .size(14.0)
                            .color(INK),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut letter)
                            .desired_width(120.0)
                            .char_limit(8)
                            .hint_text("A"),
                    );
                });
            });
        }
        let mut changed = false;
        if let Some(session) = &mut self.stamp_session {
            if session.letter != letter {
                session.letter = letter;
                changed = true;
                if let Some(stamp) = &mut session.stamp {
                    stamp.text = session.letter.clone();
                }
            }
            if let Some(kind) = picked {
                session.kind = kind;
                changed = true;
                if let Some(stamp) = &mut session.stamp {
                    stamp.kind = kind;
                    stamp.text = session.letter.clone();
                }
            }
        }
        let has_stamp = self
            .stamp_session
            .as_ref()
            .is_some_and(|session| session.stamp.is_some());
        if changed && has_stamp {
            self.apply_stamp_live();
            self.publish_stamp();
        }
        if undo {
            self.undo_stamp();
        }
        if confirm {
            self.close_stamp();
        }
        if self
            .stamp_session
            .as_ref()
            .is_some_and(|session| session.stamp.is_none() && session.kind != StampKind::Letter)
        {
            ui.painter().text(
                Pos2::new(rect.center().x, rect.bottom() - 8.0),
                Align2::CENTER_CENTER,
                word(ja, "ドラッグで置く", "Drag to place"),
                FontId::proportional(12.0),
                Color32::from_rgb(90, 98, 110),
            );
        }
    }

    pub(super) fn paint_stamp(&self, ui: &egui::Ui, rect: Rect, map: &drill_render::FieldMap) {
        let Some(session) = &self.stamp_session else {
            return;
        };
        let Some(stamp) = &session.stamp else {
            return;
        };
        let painter = ui.painter().with_clip_rect(rect);
        let corners = stamp.corners();
        let screen: Vec<Pos2> = corners
            .into_iter()
            .map(|point| screen_pos(rect, map, point))
            .collect();
        for index in 0..4 {
            painter.line_segment(
                [screen[index], screen[(index + 1) % 4]],
                Stroke::new(1.5, Color32::from_white_alpha(220)),
            );
        }
        let gap = 28.0 / map.scale.max(0.01);
        let resize = screen_pos(rect, map, stamp.resize_handle());
        let rotate = screen_pos(rect, map, stamp.rotate_handle(gap));
        let top = screen[2].lerp(screen[3], 0.5);
        painter.line_segment([top, rotate], Stroke::new(1.5, WHITE));
        painter.circle_filled(rotate, 12.0, WHITE);
        painter.circle_stroke(rotate, 12.0, Stroke::new(2.0, BLUE));
        paint_rotate_glyph(&painter, rotate);
        painter.rect_filled(
            Rect::from_center_size(resize, EVec2::splat(18.0)),
            3.0,
            WHITE,
        );
        painter.rect_stroke(
            Rect::from_center_size(resize, EVec2::splat(18.0)),
            3.0,
            Stroke::new(2.0, BLUE),
            egui::StrokeKind::Inside,
        );
    }

    fn begin_stamp_drag(&mut self, at: Point, map: &drill_render::FieldMap, _compact: bool) {
        let scale = map.scale.max(0.01);
        let tolerance = 22.0 / scale;
        let gap = 28.0 / scale;
        let existing = self
            .stamp_session
            .as_ref()
            .and_then(|session| session.stamp.clone());
        let grab = existing
            .as_ref()
            .and_then(|stamp| stamp.hit(at, tolerance, gap));
        let drag = match (grab, existing) {
            (Some(StampGrab::Rotate), Some(start)) => Drag::Rotate { start },
            (Some(StampGrab::Resize), Some(start)) => Drag::Resize { start },
            (Some(StampGrab::Body), Some(start)) => Drag::Body { start, origin: at },
            _ => Drag::Create { origin: at },
        };
        if let Some(session) = &mut self.stamp_session {
            session.drag = Some(drag);
            session.last = Some(at);
        }
    }

    fn update_stamp_drag(&mut self, at: Point) {
        let Some(session) = &self.stamp_session else {
            return;
        };
        let Some(drag) = session.drag.as_ref() else {
            return;
        };
        let kind = session.kind;
        let letter = session.letter.clone();
        let preview = match drag {
            Drag::Create { origin } => {
                if distance(*origin, at) < 0.8 {
                    None
                } else {
                    Some(Stamp::from_drag(kind, *origin, at, &letter))
                }
            }
            Drag::Body { start, origin } => Some(start.translate(Point {
                x: at.x - origin.x,
                y: at.y - origin.y,
            })),
            Drag::Resize { start } => Some(start.with_resize(at)),
            Drag::Rotate { start } => Some(start.with_rotate(at)),
        };
        if let Some(session) = &mut self.stamp_session {
            session.last = Some(at);
            if preview.is_some() {
                session.stamp = preview;
            }
        }
        if self
            .stamp_session
            .as_ref()
            .is_some_and(|session| session.stamp.is_some())
        {
            self.apply_stamp_live();
        }
    }

    fn finish_stamp_drag(
        &mut self,
        at: Option<Point>,
        map: &drill_render::FieldMap,
        compact: bool,
    ) {
        let at = at.or_else(|| self.stamp_session.as_ref().and_then(|session| session.last));
        let drag = self
            .stamp_session
            .as_mut()
            .and_then(|session| session.drag.take());
        let Some(drag) = drag else {
            return;
        };
        if let Drag::Create { origin } = drag {
            let moved = at.is_some_and(|point| distance(origin, point) >= 0.8);
            if !moved {
                self.toggle_performer(origin, map, compact);
                self.apply_stamp_live();
            }
        }
        self.publish_stamp();
    }

    fn toggle_performer(&mut self, at: Point, map: &drill_render::FieldMap, compact: bool) {
        let positions = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.positions.clone())
            .unwrap_or_default();
        let local = map.map(at);
        let Some(index) = crate::touch::pick_performer(map, &positions, local, hit_radius(compact))
        else {
            return;
        };
        let Some(session) = &mut self.stamp_session else {
            return;
        };
        if let Some(slot) = session.selected.iter().position(|&picked| picked == index) {
            session.selected.remove(slot);
        } else {
            session.selected.push(index);
            session.selected.sort_unstable();
        }
    }

    fn apply_stamp_live(&mut self) {
        let Some(session) = &self.stamp_session else {
            return;
        };
        let Some(stamp) = session.stamp.clone() else {
            return;
        };
        let next = placed_positions(&session.basis, &session.selected, &stamp);
        if let Some(set) = self.document.sets.get_mut(self.current_set) {
            set.positions = next;
        }
    }

    pub(super) fn publish_stamp(&mut self) {
        let current = self
            .document
            .sets
            .get(self.current_set)
            .map(|set| set.positions.clone())
            .unwrap_or_default();
        let selected = self
            .stamp_session
            .as_ref()
            .map(|session| session.selected.clone())
            .unwrap_or_default();
        self.publish_indexes(&selected, &current);
    }

    fn publish_indexes(&mut self, indexes: &[usize], points: &[Point]) {
        for &index in indexes {
            if let Some(&point) = points.get(index) {
                self.place_selected(index, point);
            }
        }
    }
}

pub(super) fn mode_button(ui: &mut egui::Ui, active: bool) -> egui::Response {
    kind_button_mark(ui, 44.0, active, |painter, icon| {
        let color = if active { WHITE } else { BLUE };
        let center = icon.center();
        painter.circle_stroke(center + EVec2::new(-4.0, 2.0), 3.0, Stroke::new(2.0, color));
        painter.circle_stroke(center + EVec2::new(5.0, 2.0), 3.0, Stroke::new(2.0, color));
        painter.circle_stroke(center + EVec2::new(0.5, -5.0), 3.0, Stroke::new(2.0, color));
    })
}

fn kind_button(ui: &mut egui::Ui, kind: StampKind, active: bool, side: f32) -> egui::Response {
    kind_button_mark(ui, side, active, |painter, icon| {
        let color = if active { WHITE } else { INK };
        paint_kind(painter, icon, kind, color);
    })
}

fn kind_button_mark(
    ui: &mut egui::Ui,
    side: f32,
    active: bool,
    paint: impl FnOnce(&egui::Painter, Rect),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(EVec2::splat(side), Sense::click());
    let fill = if active { BLUE } else { PAPER };
    ui.painter().rect_filled(rect, 10.0, fill);
    ui.painter().rect_stroke(
        rect,
        10.0,
        Stroke::new(1.0, Color32::from_rgb(186, 192, 200)),
        egui::StrokeKind::Inside,
    );
    paint(ui.painter(), rect.shrink(9.0));
    response
}

fn paint_kind(painter: &egui::Painter, icon: Rect, kind: StampKind, color: Color32) {
    let stroke = Stroke::new(2.0, color);
    match kind {
        StampKind::Line => {
            painter.line_segment(
                [
                    Pos2::new(icon.left(), icon.center().y),
                    Pos2::new(icon.right(), icon.center().y),
                ],
                stroke,
            );
        }
        StampKind::Circle => {
            painter.circle_stroke(
                icon.center(),
                icon.width().min(icon.height()) * 0.42,
                stroke,
            );
        }
        StampKind::Block => {
            for row in 0..2 {
                for col in 0..2 {
                    let center = Pos2::new(
                        icon.left() + icon.width() * (0.28 + col as f32 * 0.44),
                        icon.top() + icon.height() * (0.28 + row as f32 * 0.44),
                    );
                    painter.circle_filled(center, 2.5, color);
                }
            }
        }
        StampKind::Vee => {
            let apex = Pos2::new(icon.center().x, icon.bottom() - 1.0);
            painter.line_segment([Pos2::new(icon.left(), icon.top() + 1.0), apex], stroke);
            painter.line_segment([Pos2::new(icon.right(), icon.top() + 1.0), apex], stroke);
        }
        StampKind::Arc => {
            let mut previous = None;
            for step in 0..=8 {
                let t = step as f32 / 8.0;
                let x = icon.left() + icon.width() * t;
                let bow = (t * std::f32::consts::PI).sin();
                let y = icon.bottom() - 2.0 - bow * (icon.height() - 4.0);
                let point = Pos2::new(x, y);
                if let Some(previous) = previous {
                    painter.line_segment([previous, point], stroke);
                }
                previous = Some(point);
            }
        }
        StampKind::Letter => {
            painter.text(
                icon.center(),
                Align2::CENTER_CENTER,
                "A",
                FontId::proportional(icon.height().min(18.0)),
                color,
            );
        }
    }
}

fn paint_undo(painter: &egui::Painter, icon: Rect) {
    let stroke = Stroke::new(2.0, INK);
    let center = icon.center();
    let radius = icon.width().min(icon.height()) * 0.32;
    let mut previous = None;
    for step in 0..=7 {
        let angle = std::f32::consts::PI * 0.2 + std::f32::consts::PI * 1.5 * step as f32 / 7.0;
        let point = center + EVec2::new(angle.cos(), angle.sin()) * radius;
        if let Some(previous) = previous {
            painter.line_segment([previous, point], stroke);
        }
        previous = Some(point);
    }
}

fn paint_check(painter: &egui::Painter, icon: Rect) {
    let stroke = Stroke::new(2.5, WHITE);
    painter.line_segment(
        [
            Pos2::new(icon.left() + 1.0, icon.center().y),
            Pos2::new(icon.center().x - 1.0, icon.bottom() - 2.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            Pos2::new(icon.center().x - 1.0, icon.bottom() - 2.0),
            Pos2::new(icon.right() - 1.0, icon.top() + 2.0),
        ],
        stroke,
    );
}

fn paint_rotate_glyph(painter: &egui::Painter, center: Pos2) {
    let stroke = Stroke::new(1.6, BLUE);
    let mut previous = None;
    for step in 0..=6 {
        let angle = -0.4 + std::f32::consts::TAU * 0.7 * step as f32 / 6.0;
        let point = center + EVec2::new(angle.cos(), angle.sin()) * 6.0;
        if let Some(previous) = previous {
            painter.line_segment([previous, point], stroke);
        }
        previous = Some(point);
    }
}

fn distance(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

pub(super) fn school_show() -> Document {
    let mut document = Document::demo(8, 10);
    let template = document.sets[0].clone();
    let center = Point { x: 50.0, y: 26.7 };
    let kinds = [
        StampKind::Block,
        StampKind::Line,
        StampKind::Circle,
        StampKind::Vee,
        StampKind::Arc,
        StampKind::Letter,
    ];
    document.sets.clear();
    for (index, kind) in kinds.into_iter().enumerate() {
        let stamp = Stamp {
            kind,
            center,
            width: if kind == StampKind::Line { 64.0 } else { 36.0 },
            height: if kind == StampKind::Line { 8.0 } else { 30.0 },
            rotation: 0.0,
            text: "HI".into(),
        };
        let mut set = template.clone();
        set.id = SetId::new((index + 1) as u32).expect("set id");
        set.name = format!("セット {}", index + 1);
        set.positions = stamp.sample(document.performers.len());
        set.shape = None;
        set.counts = 16;
        document.sets.push(set);
    }
    document.title = "文化祭".into();
    document
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::touch::dot_radius;

    #[test]
    fn school_show_is_eighty_people_across_six_sets() {
        let document = school_show();
        assert_eq!(document.performers.len(), 80);
        assert_eq!(document.sets.len(), 6);
        for set in &document.sets {
            assert_eq!(set.positions.len(), 80);
        }
        let circle = &document.sets[2].positions;
        let center = Point { x: 50.0, y: 26.7 };
        let radii: Vec<f32> = circle
            .iter()
            .map(|point| {
                let dx = point.x - center.x;
                let dy = point.y - center.y;
                (dx * dx + dy * dy).sqrt()
            })
            .collect();
        let average = radii.iter().sum::<f32>() / radii.len() as f32;
        assert!(
            radii.iter().all(|radius| (radius - average).abs() < 0.2),
            "set 3 should be the circle stamp"
        );
    }

    #[test]
    fn crowded_cast_shrinks_the_drawn_dot() {
        assert!(dot_radius(true) >= 11.0);
        assert!(hit_radius(true) >= 44.0);
    }
}
