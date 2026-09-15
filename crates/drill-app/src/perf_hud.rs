//! A frame-pacing overlay for people who judge software by how it feels.
//!
//! The number that matters for perceived smoothness is not the average frame
//! rate, it is the tail. A "60fps" editor that drops a 40ms frame every second
//! reads as sluggish even though its average is fine, so this surfaces the p99
//! and the worst frame in the window next to the instantaneous reading, and
//! plots the whole window so a hitch is visible as a spike rather than
//! averaged away.
//!
//! Frame times are also judged against the pace this machine has actually
//! demonstrated (the 10th percentile of the window) rather than a hardcoded
//! 16.7ms: on a 165Hz panel a 16ms frame is a stutter, and on a 60Hz panel it
//! is perfect. Hardcoding 60Hz anywhere in a latency tool would be the exact
//! mistake the tool exists to catch.
//!
//! Cost discipline: this runs on every frame of the application's life, so the
//! history is a fixed-size ring buffer with no heap behind it, sampling is two
//! stores and a modulo, and nothing is formatted, sorted, or painted unless the
//! overlay is actually on screen.

use super::commands::{Command, SPECS, Shortcut};
use drill_core::Locale;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2};

/// Two seconds of history at 60Hz, half a second at 240Hz. Long enough for a
/// p99 to describe something real, short enough that a hitch you just caused
/// is still on screen when you look up at it.
const SAMPLES: usize = 120;

const PANEL_WIDTH: f32 = 236.0;
const PADDING: f32 = 12.0;
const GRAPH_HEIGHT: f32 = 46.0;
const ROW_HEIGHT: f32 = 17.0;
/// Clears the menu bar at the default style's row height and at every zoom
/// factor the app supports, since egui offsets are in points.
const TOP_INSET: f32 = 48.0;
const SIDE_INSET: f32 = 12.0;

// A diagnostic overlay is deliberately not themed. It is a heads-up display
// laid over whatever the user is working on, and it has to stay legible above
// both the dark studio themes and the light Daylight theme, so it brings its
// own high-contrast palette instead of inheriting one.
const PANEL_FILL: Color32 = Color32::from_black_alpha(214);
// `Color32::from_white_alpha` isn't `const fn`, but it is defined as exactly
// `[a, a, a, a]`, i.e. `from_rgba_premultiplied(a, a, a, a)`, which is.
const PANEL_EDGE: Color32 = Color32::from_rgba_premultiplied(38, 38, 38, 38);
const LABEL_TEXT: Color32 = Color32::from_rgb(150, 156, 166);
const VALUE_TEXT: Color32 = Color32::from_rgb(232, 236, 242);
const GRID_LINE: Color32 = Color32::from_rgba_premultiplied(30, 30, 30, 30);
const PACE_GOOD: Color32 = Color32::from_rgb(86, 214, 148);
const PACE_WARN: Color32 = Color32::from_rgb(245, 197, 66);
const PACE_BAD: Color32 = Color32::from_rgb(240, 104, 104);

/// Which paint path produced the field this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Renderer {
    /// The wgpu bridge is installed and healthy.
    Gpu,
    /// The bridge exists and is healthy but the user turned it off.
    GpuDisabled,
    /// No bridge, or the bridge reported itself unhealthy: egui's CPU painter
    /// is drawing the field.
    Cpu,
}

impl Renderer {
    fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::Gpu => super::i18n::registered(locale, "perf-hud.005"),
            Self::GpuDisabled => super::i18n::registered(locale, "perf-hud.006"),
            Self::Cpu => super::i18n::registered(locale, "perf-hud.007"),
        }
    }
}

/// Everything the overlay reports about the scene, snapshotted by the caller
/// so this module never reaches into application state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Scene {
    pub performers: usize,
    pub dots: u32,
    pub draw_commands: usize,
    pub dropped_nonfinite: u32,
    pub renderer: Renderer,
}

/// Frame-time distribution over the retained window, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Stats {
    pub last: f32,
    /// 10th percentile: the best pace this machine sustained in the window,
    /// which is the vsync interval whenever the app is repainting
    /// continuously. Used as the budget everything else is judged against.
    pub floor: f32,
    pub p50: f32,
    pub p99: f32,
    pub worst: f32,
    pub count: usize,
}

impl Stats {
    fn fps(self) -> f32 {
        if self.last > f32::EPSILON {
            1000.0 / self.last
        } else {
            0.0
        }
    }

    /// Grades a frame time against the pace the hardware has demonstrated,
    /// never against an assumed 60Hz.
    fn pace_color(self, value: f32) -> Color32 {
        let budget = self.floor.max(0.5);
        if value <= budget * 1.25 {
            PACE_GOOD
        } else if value <= budget * 2.0 {
            PACE_WARN
        } else {
            PACE_BAD
        }
    }
}

pub(crate) struct PerfHud {
    visible: bool,
    /// Frame durations in milliseconds. Written cyclically; `next` is the slot
    /// the next sample goes into, so it is also the oldest sample once the
    /// buffer has wrapped.
    samples: [f32; SAMPLES],
    next: usize,
    filled: usize,
}

impl Default for PerfHud {
    fn default() -> Self {
        Self {
            visible: false,
            samples: [0.0; SAMPLES],
            next: 0,
            filled: 0,
        }
    }
}

impl PerfHud {
    pub(crate) fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Records one frame's duration.
    ///
    /// Deliberately unconditional. Sampling costs two stores whether or not
    /// anyone is looking, and keeping the ring warm while hidden means the
    /// graph is already full of real history the instant the overlay is
    /// summoned -- including the hitch that made the user reach for it.
    pub(crate) fn record(&mut self, dt_seconds: f32) {
        self.samples[self.next] = (dt_seconds * 1000.0).clamp(0.0, 1000.0);
        self.next = (self.next + 1) % SAMPLES;
        self.filled = (self.filled + 1).min(SAMPLES);
    }

    /// Samples this frame and, if the overlay is up, paints it.
    ///
    /// Returns without touching the context when hidden, so an invisible HUD
    /// cannot keep the application awake or cost it a single allocation.
    pub(crate) fn frame(
        &mut self,
        ctx: &egui::Context,
        dt_seconds: f32,
        scene: Scene,
        locale: Locale,
    ) {
        self.record(dt_seconds);
        if !self.visible {
            return;
        }
        // A frame-time graph that only advances when something else asks for a
        // repaint is a frame-time graph that lies: it would freeze mid-spike
        // whenever the UI went idle. While the overlay is up we drive the
        // frame loop ourselves, which is also the only honest way to read a
        // steady-state pace off it.
        ctx.request_repaint();
        self.paint(ctx, self.stats(), scene, locale);
    }

    /// Distribution over the retained window.
    ///
    /// The sort runs on a stack copy of the ring, never the heap, and only
    /// from the paint path -- 120 floats is a few microseconds and it buys an
    /// exact p99 rather than a running approximation that drifts.
    pub(crate) fn stats(&self) -> Stats {
        let count = self.filled;
        if count == 0 {
            return Stats::default();
        }
        // Before the first wrap the live samples are exactly `[..filled]`;
        // after it the whole array is live. Percentiles do not care about
        // order, so no rotation is needed here.
        let mut sorted = [0.0_f32; SAMPLES];
        sorted[..count].copy_from_slice(&self.samples[..count]);
        let sorted = &mut sorted[..count];
        sorted.sort_unstable_by(f32::total_cmp);
        Stats {
            last: self.samples[(self.next + SAMPLES - 1) % SAMPLES],
            floor: percentile(sorted, 0.10),
            p50: percentile(sorted, 0.50),
            p99: percentile(sorted, 0.99),
            worst: sorted[count - 1],
            count,
        }
    }

    /// The retained window, oldest sample first.
    fn history(&self) -> impl Iterator<Item = f32> + '_ {
        let start = if self.filled == SAMPLES { self.next } else { 0 };
        (0..self.filled).map(move |offset| self.samples[(start + offset) % SAMPLES])
    }

    fn paint(&self, ctx: &egui::Context, stats: Stats, scene: Scene, locale: Locale) {
        let rows = 4 + usize::from(scene.dropped_nonfinite > 0);
        let height = PADDING * 2.0
            + 15.0  // title
            + 8.0
            + GRAPH_HEIGHT
            + 10.0
            + 25.0  // frame time / fps
            + 4.0
            + 14.0  // p99 / worst
            + 12.0  // rule
            + rows as f32 * ROW_HEIGHT;
        let screen = ctx.content_rect();
        let panel = Rect::from_min_size(
            Pos2::new(
                screen.right() - SIDE_INSET - PANEL_WIDTH,
                screen.top() + TOP_INSET,
            ),
            Vec2::new(PANEL_WIDTH, height),
        );
        // A pure overlay: painted straight onto a foreground layer rather than
        // through an Area, so it never participates in layout, never steals a
        // click from the field beneath it, and cannot be dragged out of the
        // way by accident.
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("drillforge-perf-hud"),
        ));
        painter.rect_filled(panel, 8.0, PANEL_FILL);
        painter.rect_stroke(panel, 8.0, Stroke::new(1.0, PANEL_EDGE), StrokeKind::Inside);

        let content = panel.shrink(PADDING);
        let mut y = content.top();

        painter.text(
            Pos2::new(content.left(), y),
            Align2::LEFT_TOP,
            super::i18n::registered(locale, "perf-hud.001"),
            FontId::proportional(12.0),
            VALUE_TEXT,
        );
        if let Some(chord) = toggle_chord() {
            painter.text(
                Pos2::new(content.right(), y),
                Align2::RIGHT_TOP,
                ctx.format_shortcut(&chord.value()),
                FontId::proportional(11.0),
                LABEL_TEXT,
            );
        }
        y += 15.0 + 8.0;

        let graph = Rect::from_min_size(
            Pos2::new(content.left(), y),
            Vec2::new(content.width(), GRAPH_HEIGHT),
        );
        self.paint_graph(&painter, graph, stats);
        y += GRAPH_HEIGHT + 10.0;

        painter.text(
            Pos2::new(content.left(), y),
            Align2::LEFT_TOP,
            format!("{:.1} ms", stats.last),
            FontId::proportional(20.0),
            stats.pace_color(stats.last),
        );
        painter.text(
            Pos2::new(content.right(), y),
            Align2::RIGHT_TOP,
            format!("{:.0} fps", stats.fps()),
            FontId::proportional(20.0),
            VALUE_TEXT,
        );
        y += 25.0 + 4.0;

        // p99 is the headline number here, not the average: it is what decides
        // whether a drag feels glued to the cursor or not.
        painter.text(
            Pos2::new(content.left(), y),
            Align2::LEFT_TOP,
            format!("p99 {:.1} ms", stats.p99),
            FontId::proportional(11.0),
            stats.pace_color(stats.p99),
        );
        painter.text(
            Pos2::new(content.right(), y),
            Align2::RIGHT_TOP,
            format!("max {:.1} ms", stats.worst),
            FontId::proportional(11.0),
            stats.pace_color(stats.worst),
        );
        y += 14.0 + 6.0;

        painter.line_segment(
            [Pos2::new(content.left(), y), Pos2::new(content.right(), y)],
            Stroke::new(1.0, PANEL_EDGE),
        );
        y += 6.0;

        let mut row = |label: &str, value: String, color: Color32| {
            painter.text(
                Pos2::new(content.left(), y),
                Align2::LEFT_TOP,
                label,
                FontId::proportional(11.0),
                LABEL_TEXT,
            );
            // Right-anchored so a changing digit count never shifts the
            // column and makes the panel look like it is twitching.
            painter.text(
                Pos2::new(content.right(), y),
                Align2::RIGHT_TOP,
                value,
                FontId::proportional(11.0),
                color,
            );
            y += ROW_HEIGHT;
        };
        row(
            super::i18n::registered(locale, "perf-hud.002"),
            scene.performers.to_string(),
            VALUE_TEXT,
        );
        row(
            super::i18n::registered(locale, "perf-hud.003"),
            scene.dots.to_string(),
            VALUE_TEXT,
        );
        row(
            super::i18n::registered(locale, "perf-hud.004"),
            scene.draw_commands.to_string(),
            VALUE_TEXT,
        );
        row(
            super::i18n::registered(locale, "perf-hud.008"),
            scene.renderer.label(locale).to_owned(),
            if scene.renderer == Renderer::Gpu {
                PACE_GOOD
            } else {
                PACE_WARN
            },
        );
        if scene.dropped_nonfinite > 0 {
            row(
                super::i18n::registered(locale, "perf-hud.009"),
                scene.dropped_nonfinite.to_string(),
                PACE_BAD,
            );
        }
    }

    fn paint_graph(&self, painter: &egui::Painter, graph: Rect, stats: Stats) {
        painter.rect_filled(graph, 4.0, Color32::from_black_alpha(90));
        if stats.count == 0 {
            return;
        }
        // Headroom above the worst frame keeps the plot from pinning to the
        // ceiling, and the floor term stops a perfectly steady window from
        // magnifying sub-microsecond noise into dramatic-looking spikes.
        let ceiling = stats.worst.max(stats.floor * 2.5).max(1.0);
        // The demonstrated pace, drawn as the line frames should stay under.
        let budget_y = graph.bottom() - (stats.floor / ceiling) * graph.height();
        painter.line_segment(
            [
                Pos2::new(graph.left(), budget_y),
                Pos2::new(graph.right(), budget_y),
            ],
            Stroke::new(1.0, GRID_LINE),
        );

        let step = graph.width() / SAMPLES as f32;
        let bar = (step - 0.4).max(0.8);
        // One quad per sample. 120 tiny rects is nothing next to the widget
        // tree this sits on top of, and it keeps every spike individually
        // visible instead of smoothing the tail away into a polyline.
        for (index, sample) in self.history().enumerate() {
            let normalized = (sample / ceiling).clamp(0.0, 1.0);
            let top = graph.bottom() - normalized * graph.height();
            let left = graph.left() + index as f32 * step;
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(left, top), Pos2::new(left + bar, graph.bottom())),
                0.0,
                stats.pace_color(sample),
            );
        }
    }
}

fn percentile(sorted: &[f32], quantile: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let last = sorted.len() - 1;
    let index = (last as f32 * quantile).round() as usize;
    sorted[index.min(last)]
}

/// Reads the chord back out of the authoritative command table so the hint
/// printed in the corner can never drift from the key that actually works.
fn toggle_chord() -> Option<Shortcut> {
    SPECS
        .iter()
        .find(|spec| spec.command == Command::TogglePerfHud)
        .and_then(|spec| spec.shortcut)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hud_with(samples_ms: &[f32]) -> PerfHud {
        let mut hud = PerfHud::default();
        for &ms in samples_ms {
            hud.record(ms / 1000.0);
        }
        hud
    }

    #[test]
    fn empty_window_reports_zeroes_instead_of_dividing_by_nothing() {
        let stats = PerfHud::default().stats();
        assert_eq!(stats.count, 0);
        assert_eq!(stats.fps(), 0.0);
        assert_eq!(stats.worst, 0.0);
    }

    #[test]
    fn ring_buffer_retains_exactly_the_last_window_and_never_grows() {
        let mut hud = PerfHud::default();
        for index in 0..SAMPLES * 3 {
            hud.record(index as f32 / 1000.0);
        }
        assert_eq!(hud.stats().count, SAMPLES);
        let history: Vec<f32> = hud.history().collect();
        assert_eq!(history.len(), SAMPLES);
        // Oldest-first ordering must survive wrapping, otherwise the graph
        // would scroll with a seam in it. Values round-trip through the
        // seconds -> milliseconds conversion inside `record`, so compare
        // with f32 tolerance rather than bit-exact equality.
        for (offset, actual) in history.iter().enumerate() {
            let expected = (SAMPLES * 2 + offset) as f32;
            assert!(
                (actual - expected).abs() < 0.01,
                "sample {offset}: expected {expected}, got {actual}"
            );
        }
        assert!((hud.stats().last - (SAMPLES * 3 - 1) as f32).abs() < 0.01);
    }

    #[test]
    fn tail_statistics_order_correctly_and_a_single_spike_survives_averaging() {
        // 119 good frames and one 40ms hitch: the mean is unremarkable, and
        // the tail is exactly what the overlay exists to expose.
        let mut samples = vec![6.9_f32; SAMPLES - 1];
        samples.push(40.0);
        let stats = hud_with(&samples).stats();
        assert_eq!(stats.count, SAMPLES);
        assert!(stats.floor <= stats.p50, "floor must not exceed the median");
        assert!(stats.p50 <= stats.p99);
        assert!(stats.p99 <= stats.worst);
        assert_eq!(stats.worst, 40.0);
        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        assert!(
            mean < 8.0,
            "the mean should look fine, which is the whole point"
        );
    }

    #[test]
    fn pace_is_graded_against_demonstrated_refresh_not_a_hardcoded_60hz() {
        // A 240Hz machine holding 4.1ms frames: a 16ms frame here is a stall,
        // and grading it against 16.7ms would have called it perfect.
        let fast = hud_with(&[4.1_f32; SAMPLES]).stats();
        assert!(fast.floor < 5.0);
        assert_eq!(fast.pace_color(4.2), PACE_GOOD);
        assert_eq!(fast.pace_color(16.0), PACE_BAD);
        // The same 16ms frame on a 60Hz machine is a healthy frame.
        let slow = hud_with(&[16.6_f32; SAMPLES]).stats();
        assert_eq!(slow.pace_color(16.0), PACE_GOOD);
    }

    #[test]
    fn frame_time_converts_to_the_expected_refresh_rate() {
        let stats = hud_with(&[1000.0 / 144.0; SAMPLES]).stats();
        assert!(
            (stats.fps() - 144.0).abs() < 0.5,
            "expected ~144 fps, got {}",
            stats.fps()
        );
    }

    #[test]
    fn hidden_overlay_paints_nothing_and_lets_the_app_sleep() {
        let ctx = egui::Context::default();
        let mut hud = PerfHud::default();
        // A brand-new `Context`'s very first frame can request an immediate
        // repaint on its own (font atlas / initial layout settling)
        // regardless of anything this HUD does. What this test cares about
        // is steady state: does a *hidden* HUD keep an already-running app
        // awake. So warm the context up first and assert on the frame after.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            hud.frame(ui.ctx(), 1.0 / 120.0, scene(), Locale::En);
        });
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            hud.frame(ui.ctx(), 1.0 / 120.0, scene(), Locale::En);
        });
        assert!(!hud.visible);
        assert!(
            output.shapes.is_empty(),
            "a hidden HUD must not paint anything"
        );
        let delay = output
            .viewport_output
            .values()
            .map(|viewport| viewport.repaint_delay)
            .min()
            .unwrap();
        assert!(
            delay > std::time::Duration::ZERO,
            "a hidden HUD must not keep the frame loop spinning"
        );
        // Sampling still happened both frames, so the graph is populated the
        // instant the overlay is revealed.
        assert_eq!(hud.stats().count, 2);
    }

    #[test]
    fn visible_overlay_paints_and_drives_the_frame_loop() {
        let ctx = egui::Context::default();
        let mut hud = PerfHud::default();
        hud.toggle();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            hud.frame(ui.ctx(), 1.0 / 120.0, scene(), Locale::En);
        });
        assert!(hud.visible);
        assert!(!output.shapes.is_empty());
        let delay = output
            .viewport_output
            .values()
            .map(|viewport| viewport.repaint_delay)
            .min()
            .unwrap();
        assert_eq!(
            delay,
            std::time::Duration::ZERO,
            "a visible graph must repaint every frame or it freezes mid-spike"
        );
    }

    #[test]
    fn every_reported_string_is_translated_in_both_locales() {
        for id in [
            "perf-hud.001",
            "perf-hud.002",
            "perf-hud.003",
            "perf-hud.004",
            "perf-hud.005",
            "perf-hud.006",
            "perf-hud.007",
            "perf-hud.008",
            "perf-hud.009",
        ] {
            for locale in [Locale::Ja, Locale::En] {
                let value = super::super::i18n::registered(locale, id);
                assert_ne!(
                    value, "[missing message]",
                    "untranslated {id} in {locale:?}"
                );
                assert!(!value.is_empty());
            }
        }
    }

    #[test]
    fn the_advertised_chord_is_the_one_the_command_table_dispatches() {
        let chord = toggle_chord().expect("the overlay must be reachable from the command table");
        assert_eq!(chord, Shortcut::CommandShift(egui::Key::P));
    }

    fn scene() -> Scene {
        Scene {
            performers: 64,
            dots: 64,
            draw_commands: 312,
            dropped_nonfinite: 0,
            renderer: Renderer::Gpu,
        }
    }
}
