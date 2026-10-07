//! Plain-language checks for the move leaving the current scene.
//!
//! The clinic already counts collisions and long strides. These helpers turn
//! that into names, a count, and a step size a rehearsal can say out loud.
//! Nothing here writes the document.

use super::{DrillApp, editing, i18n};
use drill_core::clinic::{self, CollisionEvent};
use drill_core::{Document, Locale, Point};
use eframe::egui;
use std::collections::BTreeSet;

const SHOWN_LINES: usize = 3;
const KEPT: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CloseCall {
    pub a: usize,
    pub b: usize,
    pub beat: u32,
    distance: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BigStep {
    pub index: usize,
    pub counts: u16,
    pub steps: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MoveCheck {
    pub close: Vec<CloseCall>,
    pub big: Vec<BigStep>,
}

impl DrillApp {
    pub(super) fn refresh_move_check(&mut self) {
        let set_index = self.current_set;
        let collisions = {
            let report = clinic::scan_transition(
                &self.document,
                set_index,
                super::smart_transition_state::clinic_params(),
                &mut self.clinic_scratch,
            );
            report.collisions.to_vec()
        };
        self.move_check = MoveCheck {
            close: close_calls(&self.document, set_index, &collisions),
            big: big_steps(&self.document, set_index),
        };
    }

    pub(super) fn save_scene_diagram(&mut self) {
        let bytes = drill_export::scene_png::scene_diagram_png(&self.document, self.current_set);
        let name = format!("set-{}.png", self.current_set + 1);
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name(&name)
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, bytes) {
            Ok(()) => {
                self.status = format!(
                    "{}: {}",
                    i18n::registered(self.locale, "rehearsal.003"),
                    path.display()
                );
            }
            Err(error) => {
                self.status = format!(
                    "{}: {error}",
                    i18n::registered(self.locale, "rehearsal.004")
                );
            }
        }
    }

    pub(super) fn show_plain_move_warnings(&mut self, ui: &mut egui::Ui) {
        if ui
            .button(i18n::registered(self.locale, "rehearsal.001"))
            .on_hover_text(i18n::registered(self.locale, "rehearsal.002"))
            .clicked()
        {
            self.save_scene_diagram();
        }
        self.refresh_move_check();
        let close = self.move_check.close.clone();
        let big = self.move_check.big.clone();
        if close.is_empty() && big.is_empty() {
            return;
        }
        ui.add_space(4.0);
        ui.label(egui::RichText::new(i18n::registered(self.locale, "rehearsal.019")).strong());
        ui.small(i18n::registered(self.locale, "rehearsal.020"));
        let locale = self.locale;
        let close_lines: Vec<(usize, usize, String)> = close
            .iter()
            .take(SHOWN_LINES)
            .map(|call| {
                (
                    call.a,
                    call.b,
                    close_call_text(locale, &self.document, call),
                )
            })
            .collect();
        let big_lines: Vec<(usize, String)> = big
            .iter()
            .take(SHOWN_LINES)
            .map(|step| (step.index, big_step_text(locale, &self.document, step)))
            .collect();
        let close_extra = close.len().saturating_sub(SHOWN_LINES);
        let big_extra = big.len().saturating_sub(SHOWN_LINES);
        for (a, b, line) in close_lines {
            if ui
                .button(egui::RichText::new(line).color(egui::Color32::from_rgb(176, 64, 48)))
                .on_hover_text(i18n::registered(self.locale, "rehearsal.013"))
                .clicked()
            {
                self.focus_checked_people(&[a, b]);
                self.status = i18n::registered(self.locale, "rehearsal.017").into();
            }
        }
        if close_extra > 0 {
            ui.small(more_text(self.locale, close_extra));
        }
        for (index, line) in big_lines {
            if ui
                .button(egui::RichText::new(line).color(egui::Color32::from_rgb(168, 104, 24)))
                .on_hover_text(i18n::registered(self.locale, "rehearsal.015"))
                .clicked()
            {
                self.focus_checked_people(&[index]);
                self.status = i18n::registered(self.locale, "rehearsal.018").into();
            }
        }
        if big_extra > 0 {
            ui.small(more_text(self.locale, big_extra));
        }
    }

    pub(super) fn show_shortcut_help(&mut self, ctx: &egui::Context) {
        if !self.shortcut_help_open {
            return;
        }
        let mut open = true;
        egui::Window::new(i18n::registered(self.locale, "rehearsal.007"))
            .open(&mut open)
            .default_width(440.0)
            .resizable(true)
            .scroll(true)
            .show(ctx, |ui| {
                ui.set_min_width(280.0);
                ui.label(i18n::registered(self.locale, "rehearsal.008"));
                ui.separator();
                let locale = self.locale;
                for spec in super::commands::SPECS {
                    let Some(shortcut) = spec.shortcut else {
                        continue;
                    };
                    ui.label(format!(
                        "{}    {}",
                        spec.command.label(locale),
                        ui.ctx().format_shortcut(&shortcut.value())
                    ));
                }
            });
        self.shortcut_help_open = open;
    }

    fn focus_checked_people(&mut self, indices: &[usize]) {
        let next: BTreeSet<usize> = indices
            .iter()
            .copied()
            .filter(|index| *index < self.document.performers.len())
            .collect();
        if next.is_empty() {
            return;
        }
        self.replace_selection(next);
        let points = self.selected_points();
        if points.is_empty() {
            return;
        }
        self.field_viewport.center = editing::centroid(&points);
        self.field_viewport
            .set_zoom(self.field_viewport.zoom.max(1.8));
        self.field_viewport.stop_glide();
    }
}

pub(crate) fn close_calls(
    document: &Document,
    set_index: usize,
    collisions: &[CollisionEvent],
) -> Vec<CloseCall> {
    let counts = document
        .sets
        .get(set_index)
        .map(|set| u32::from(set.counts.max(1)))
        .unwrap_or(1);
    let mut calls = Vec::new();
    for event in collisions {
        let Some(a) = performer_index(document, event.a) else {
            continue;
        };
        let Some(b) = performer_index(document, event.b) else {
            continue;
        };
        let beat = (event.count.floor() as u32)
            .saturating_add(1)
            .clamp(1, counts);
        calls.push(CloseCall {
            a,
            b,
            beat,
            distance: event.distance,
        });
    }
    calls.sort_by(|left, right| {
        left.distance
            .total_cmp(&right.distance)
            .then(left.a.cmp(&right.a))
            .then(left.b.cmp(&right.b))
    });
    calls.truncate(KEPT);
    calls
}

pub(crate) fn big_steps(document: &Document, set_index: usize) -> Vec<BigStep> {
    let Some(from) = document.sets.get(set_index) else {
        return Vec::new();
    };
    let Some(to) = document.sets.get(set_index + 1) else {
        return Vec::new();
    };
    let counts = from.counts.max(1);
    let limit = f32::from(counts);
    let people = document
        .performers
        .len()
        .min(from.positions.len())
        .min(to.positions.len());
    let mut steps = Vec::new();
    for index in 0..people {
        let travel = travel_steps(&document.grid, from.positions[index], to.positions[index]);
        if travel > limit + 0.01 {
            steps.push(BigStep {
                index,
                counts,
                steps: travel,
            });
        }
    }
    steps.sort_by(|left, right| {
        right
            .steps
            .total_cmp(&left.steps)
            .then(left.index.cmp(&right.index))
    });
    steps.truncate(KEPT);
    steps
}

pub(crate) fn close_call_text(locale: Locale, document: &Document, call: &CloseCall) -> String {
    i18n::registered(locale, "rehearsal.012")
        .replace("{0}", &person_name(document, call.a))
        .replace("{1}", &person_name(document, call.b))
        .replace("{2}", &call.beat.to_string())
}

pub(crate) fn big_step_text(locale: Locale, document: &Document, step: &BigStep) -> String {
    i18n::registered(locale, "rehearsal.014")
        .replace("{0}", &person_name(document, step.index))
        .replace("{1}", &step.counts.to_string())
        .replace("{2}", &format_steps(step.steps))
}

fn more_text(locale: Locale, extra: usize) -> String {
    i18n::registered(locale, "rehearsal.016").replace("{0}", &extra.to_string())
}

fn person_name(document: &Document, index: usize) -> String {
    document
        .performers
        .get(index)
        .map(|performer| performer.label.trim())
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| (index + 1).to_string())
}

fn performer_index(document: &Document, id: drill_core::PerformerId) -> Option<usize> {
    document
        .performers
        .iter()
        .position(|performer| performer.id == id)
}

fn travel_steps(grid: &drill_core::GridConfig, start: Point, end: Point) -> f32 {
    let hstep = grid.horizontal_units / f32::from(grid.horizontal_steps.max(1));
    if !hstep.is_finite() || hstep <= f32::EPSILON {
        return 0.0;
    }
    let raw = (end.x - start.x).hypot(end.y - start.y) / hstep;
    if !raw.is_finite() {
        return 0.0;
    }
    (raw * 4.0).round() / 4.0
}

fn format_steps(steps: f32) -> String {
    let quarter = (steps * 4.0).round() / 4.0;
    if (quarter - quarter.round()).abs() < 0.01 {
        format!("{}", quarter.round() as i32)
    } else if (quarter * 2.0 - (quarter * 2.0).round()).abs() < 0.01 {
        format!("{quarter:.1}")
    } else {
        format!("{quarter:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drill_core::Point;

    fn crossing_pair() -> Document {
        let mut document = Document::demo(1, 2);
        document.sets[0].counts = 8;
        document.sets[0].positions[0] = Point { x: 10.0, y: 20.0 };
        document.sets[0].positions[1] = Point { x: 30.0, y: 20.0 };
        document.sets[1].positions[0] = Point { x: 30.0, y: 20.0 };
        document.sets[1].positions[1] = Point { x: 10.0, y: 20.0 };
        document.performers[0].label = "A1".into();
        document.performers[1].label = "B1".into();
        document
    }

    #[test]
    fn names_two_people_who_pass_too_close() {
        let document = crossing_pair();
        let mut scratch = clinic::ScanScratch::default();
        let report = clinic::scan_transition(
            &document,
            0,
            super::super::smart_transition_state::clinic_params(),
            &mut scratch,
        );
        let calls = close_calls(&document, 0, report.collisions);
        assert!(!calls.is_empty(), "crossing paths should be too close");
        assert!((1..=8).contains(&calls[0].beat));
        let japanese = close_call_text(Locale::Ja, &document, &calls[0]);
        let english = close_call_text(Locale::En, &document, &calls[0]);
        assert!(japanese.contains("A1"));
        assert!(japanese.contains("B1"));
        assert!(japanese.contains("近すぎ"));
        assert!(english.contains("too close"));
        assert_ne!(japanese, english);
    }

    #[test]
    fn flags_a_step_larger_than_the_count() {
        let mut document = Document::demo(1, 1);
        document.sets[0].counts = 4;
        document.sets[0].positions[0] = Point { x: 10.0, y: 10.0 };
        document.sets[1].positions[0] = Point { x: 30.0, y: 10.0 };
        document.performers[0].label = "C1".into();
        let steps = big_steps(&document, 0);
        assert_eq!(steps.len(), 1);
        assert!(steps[0].steps > 4.0);
        assert_eq!(steps[0].counts, 4);
        let text = big_step_text(Locale::Ja, &document, &steps[0]);
        assert!(text.contains("C1"));
        assert!(text.contains("4拍"));
        assert!(text.contains("大きすぎ"));
        assert!(big_step_text(Locale::En, &document, &steps[0]).contains("too"));
    }

    #[test]
    fn a_quiet_move_and_the_last_scene_have_nothing_to_flag() {
        let mut document = Document::demo(1, 2);
        document.sets[1].positions = document.sets[0].positions.clone();
        let mut scratch = clinic::ScanScratch::default();
        let report = clinic::scan_transition(
            &document,
            0,
            super::super::smart_transition_state::clinic_params(),
            &mut scratch,
        );
        assert!(close_calls(&document, 0, report.collisions).is_empty());
        assert!(big_steps(&document, 0).is_empty());
        assert!(big_steps(&document, document.sets.len() - 1).is_empty());
    }

    #[test]
    fn one_step_per_count_is_not_too_big() {
        let mut document = Document::demo(1, 1);
        document.sets[0].counts = 8;
        let hstep =
            document.grid.horizontal_units / f32::from(document.grid.horizontal_steps.max(1));
        document.sets[0].positions[0] = Point { x: 10.0, y: 10.0 };
        document.sets[1].positions[0] = Point {
            x: 10.0 + hstep * 8.0,
            y: 10.0,
        };
        assert!(big_steps(&document, 0).is_empty());
    }
}
