use drill_core::{Document, Edit, ProductionMarker, ProductionMarkerId, ProductionMarkerKind};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

#[derive(Default)]
pub(crate) struct TimelineChange {
    pub(crate) seek: Option<(usize, f32)>,
    pub(crate) range: Option<(u32, u32)>,
    pub(crate) viewport_interacted: bool,
    pub(crate) marker_edit: Option<Edit>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TimelineViewport {
    pub(crate) start: f32,
    pub(crate) span: f32,
}

impl TimelineViewport {
    const MIN_SPAN: f32 = 4.0;

    pub(crate) fn fit(total: u32) -> Self {
        Self {
            start: 0.0,
            span: total.max(1) as f32,
        }
    }

    pub(crate) fn normalize(&mut self, total: u32) {
        let total = total.max(1) as f32;
        self.span = self.span.clamp(Self::MIN_SPAN.min(total), total);
        self.start = self.start.clamp(0.0, (total - self.span).max(0.0));
    }

    pub(crate) fn visible_range(self) -> std::ops::RangeInclusive<f32> {
        self.start..=self.start + self.span
    }

    fn count_to_x(self, count: f32, left: f32, width: f32) -> f32 {
        left + (count - self.start) / self.span.max(f32::EPSILON) * width
    }

    pub(crate) fn x_to_count(self, x: f32, left: f32, width: f32) -> f32 {
        self.start + ((x - left) / width.max(f32::EPSILON)).clamp(0.0, 1.0) * self.span
    }

    pub(crate) fn zoom_at(&mut self, factor: f32, focus: f32, total: u32) {
        let old_span = self.span;
        let focus_ratio = ((focus - self.start) / old_span.max(f32::EPSILON)).clamp(0.0, 1.0);
        self.span *= factor;
        self.normalize(total);
        self.start = focus - focus_ratio * self.span;
        self.normalize(total);
    }

    pub(crate) fn pan(&mut self, delta_counts: f32, total: u32) {
        self.start += delta_counts;
        self.normalize(total);
    }

    pub(crate) fn contains_with_margin(self, count: f32, margin_ratio: f32) -> bool {
        let margin = self.span * margin_ratio.clamp(0.0, 0.49);
        count >= self.start + margin && count <= self.start + self.span - margin
    }

    /// The `start` that would put `count` in the middle of the viewport, after
    /// clamping to the range `normalize` would allow. Clamping here (rather
    /// than leaving it to `normalize` afterwards) is what lets
    /// [`Self::glide_start_toward`] recognise that it has arrived when the
    /// requested centre is off either end of the show.
    pub(crate) fn settled_center_start(self, count: f32, total: u32) -> f32 {
        self.clamp_start(count - self.span * 0.5, total)
    }

    fn clamp_start(self, start: f32, total: u32) -> f32 {
        let total = total.max(1) as f32;
        let span = self.span.clamp(Self::MIN_SPAN.min(total), total);
        start.clamp(0.0, (total - span).max(0.0))
    }

    /// Eases `start` toward `target_start` and reports whether it is still
    /// moving.
    ///
    /// Playback follow used to `center_on` the instant the playhead crossed
    /// the margin, which is a discontinuity dropped on the user at the exact
    /// moment they are watching motion. This glides instead.
    ///
    /// The step is the dt-correct exponential `1 - e^(-dt/tau)`, not the
    /// common `x += (target - x) * k`: the latter is a per-*frame* fraction
    /// and so converges twice as fast at 120Hz as at 60Hz, i.e. the feel of
    /// the app would depend on the monitor. This form is identical at any
    /// refresh rate.
    pub(crate) fn glide_start_toward(&mut self, target_start: f32, dt: f32, total: u32) -> bool {
        /// Time to close ~63% of the remaining distance. Chosen so a typical
        /// recentre is visually done inside ~200ms.
        const TAU_SECONDS: f32 = 0.07;
        /// Sub-pixel at any realistic zoom; below this, snap and stop so the
        /// app can go idle instead of chasing an asymptote forever.
        const SETTLE_COUNTS: f32 = 0.02;

        let target_start = self.clamp_start(target_start, total);
        let delta = target_start - self.start;
        if delta.abs() <= SETTLE_COUNTS || dt <= 0.0 {
            self.start = target_start;
            self.normalize(total);
            return false;
        }
        self.start += delta * (1.0 - (-dt / TAU_SECONDS).exp());
        self.normalize(total);
        true
    }
}

pub(crate) fn snapped_viewport_count(
    pointer_x: f32,
    left: f32,
    width: f32,
    viewport: TimelineViewport,
    total: u32,
    boundaries: impl Iterator<Item = u32>,
) -> u32 {
    let raw = viewport
        .x_to_count(pointer_x, left, width)
        .clamp(0.0, total as f32);
    let integer = raw.round() as u32;
    let boundary_threshold = 7.0 / width.max(1.0) * viewport.span;
    boundaries
        .filter(|boundary| (*boundary as f32 - raw).abs() <= boundary_threshold)
        .min_by(|a, b| (*a as f32 - raw).abs().total_cmp(&(*b as f32 - raw).abs()))
        .unwrap_or(integer)
        .min(total)
}

/// Selects a readable count interval while bounding paint work by viewport width.
pub(crate) fn timeline_tick_stride(total_counts: u32, width: f32) -> u32 {
    let desired_ticks = (width / 9.0).max(1.0) as u32;
    let minimum = total_counts.div_ceil(desired_ticks).max(1);
    [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024]
        .into_iter()
        .find(|step| *step >= minimum)
        .unwrap_or_else(|| minimum.checked_next_power_of_two().unwrap_or(u32::MAX))
}

/// Deterministic label placement used by both painting and UI regression tests.
///
/// Text is deliberately assigned to rows when horizontal bounds overlap. This
/// keeps IN, OUT and NOW readable at narrow logical widths and high DPI instead
/// of relying on font clipping to hide collisions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TimelineLabelLayout {
    pub(crate) in_pos: Pos2,
    pub(crate) out_pos: Pos2,
    pub(crate) now_pos: Pos2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TimelineWaypoint {
    pub(crate) set_index: usize,
    pub(crate) count: u32,
    pub(crate) label: String,
    pub(crate) detail: String,
}

/// Existing production annotations are already persistent, undoable set-start
/// landmarks. Present them as timeline waypoints rather than introducing a
/// second marker model that could drift from rehearsal paperwork.
pub(crate) fn timeline_waypoints(document: &Document) -> Vec<TimelineWaypoint> {
    let mut count = 0_u32;
    document
        .sets
        .iter()
        .enumerate()
        .filter_map(|(set_index, set)| {
            let annotation = &set.annotation;
            let label = if !annotation.rehearsal_mark.trim().is_empty() {
                annotation.rehearsal_mark.trim().to_owned()
            } else if !annotation.title.trim().is_empty() {
                annotation.title.trim().to_owned()
            } else {
                String::new()
            };
            let current = count;
            count = count.saturating_add(u32::from(set.counts));
            (!label.is_empty()).then(|| TimelineWaypoint {
                set_index,
                count: current,
                detail: if annotation.notes.trim().is_empty() {
                    set.name.clone()
                } else {
                    annotation.notes.trim().to_owned()
                },
                label,
            })
        })
        .collect()
}

fn marker_kind_name(kind: ProductionMarkerKind, locale: drill_core::Locale) -> &'static str {
    match kind {
        ProductionMarkerKind::Hit => super::i18n::registered(locale, "timeline.010"),
        ProductionMarkerKind::Rehearsal => super::i18n::registered(locale, "timeline.011"),
        ProductionMarkerKind::Note => super::i18n::registered(locale, "timeline.012"),
    }
}

fn next_marker_id(document: &Document) -> Option<ProductionMarkerId> {
    document
        .production_markers
        .iter()
        .map(|marker| marker.id.get())
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .and_then(ProductionMarkerId::new)
}

/// Finds the adjacent production landmark around an exact global count.
/// Markers at the current count are deliberately skipped: repeated navigation
/// always advances instead of getting stuck on the same rehearsal event.
pub(crate) fn adjacent_production_marker_count(
    document: &Document,
    current_count: u32,
    forward: bool,
) -> Option<u32> {
    if forward {
        document
            .production_markers
            .iter()
            .filter(|marker| marker.count > current_count)
            .map(|marker| marker.count)
            .min()
    } else {
        document
            .production_markers
            .iter()
            .filter(|marker| marker.count < current_count)
            .map(|marker| marker.count)
            .max()
    }
}

impl TimelineLabelLayout {
    pub(crate) fn calculate(
        rect: Rect,
        range_left: f32,
        range_right: f32,
        playhead_x: f32,
        playback_start: u32,
        playback_end: u32,
        global: f32,
    ) -> Self {
        // egui's monospace glyph advance at these font sizes is below 7 pt.
        // Add padding so this remains conservative across platform fonts.
        let text_width = |characters: usize| characters as f32 * 7.0 + 4.0;
        let in_width = text_width(format!("▶ IN {playback_start}").chars().count());
        let out_width = text_width(format!("OUT {playback_end} ◀").chars().count());
        let now_width = text_width(format!("NOW {global:.2}").chars().count());

        let in_x = (range_left + 5.0).clamp(rect.left() + 5.0, rect.right() - in_width);
        let out_x = (range_right - 5.0).clamp(rect.left() + out_width, rect.right() - 5.0);
        let in_bounds = (in_x, in_x + in_width);
        let out_bounds = (out_x - out_width, out_x);
        let ranges_overlap = in_bounds.1 + 4.0 > out_bounds.0;
        let out_y = rect.top() + if ranges_overlap { 18.0 } else { 3.0 };

        let now_x = playhead_x.clamp(
            rect.left() + now_width * 0.5,
            rect.right() - now_width * 0.5,
        );
        let now_bounds = (now_x - now_width * 0.5, now_x + now_width * 0.5);
        let now_hits_in = now_bounds.1 + 4.0 > in_bounds.0 && now_bounds.0 < in_bounds.1 + 4.0;
        let now_hits_out = !ranges_overlap
            && now_bounds.1 + 4.0 > out_bounds.0
            && now_bounds.0 < out_bounds.1 + 4.0;
        let now_y = rect.top()
            + if now_hits_in || now_hits_out {
                33.0
            } else {
                3.0
            };

        Self {
            in_pos: Pos2::new(in_x, rect.top() + 3.0),
            out_pos: Pos2::new(out_x, out_y),
            now_pos: Pos2::new(now_x, now_y),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_count_track(
    ui: &mut egui::Ui,
    document: &Document,
    current_set: usize,
    local_count: f32,
    playback_start: u32,
    playback_end: u32,
    viewport: &mut TimelineViewport,
    locale: drill_core::Locale,
) -> TimelineChange {
    let mut change = TimelineChange::default();
    let total_counts = document.timeline_counts();
    let total = total_counts.max(1) as f32;
    viewport.normalize(total_counts);
    let global = document
        .global_count(current_set, local_count)
        .clamp(0.0, total);
    let (response, painter) = ui.allocate_painter(
        Vec2::new(ui.available_width(), 84.0),
        Sense::click_and_drag(),
    );
    let waypoint_lane = Rect::from_min_max(
        response.rect.left_top() + Vec2::new(8.0, 5.0),
        Pos2::new(response.rect.right() - 8.0, response.rect.top() + 25.0),
    );
    let rect = Rect::from_min_max(
        Pos2::new(response.rect.left() + 8.0, response.rect.top() + 29.0),
        response.rect.right_bottom() - Vec2::new(8.0, 5.0),
    );
    painter.rect_filled(waypoint_lane, 5.0, Color32::from_rgb(22, 29, 40));
    painter.rect_filled(rect, 5.0, Color32::from_rgb(18, 23, 31));

    let scroll = ui.input(|input| input.smooth_scroll_delta);
    if response.hovered() && scroll != Vec2::ZERO {
        change.viewport_interacted = true;
        if ui.input(|input| input.modifiers.ctrl) {
            let focus_x = response.hover_pos().map_or(rect.center().x, |p| p.x);
            let focus = viewport.x_to_count(focus_x, rect.left(), rect.width());
            viewport.zoom_at((-scroll.y * 0.0025).exp(), focus, total_counts);
        } else {
            let pixels = if scroll.x.abs() > scroll.y.abs() {
                scroll.x
            } else {
                scroll.y
            };
            viewport.pan(
                -pixels / rect.width().max(1.0) * viewport.span,
                total_counts,
            );
        }
    }
    if response.dragged_by(egui::PointerButton::Middle) {
        change.viewport_interacted = true;
        viewport.pan(
            -response.drag_delta().x / rect.width().max(1.0) * viewport.span,
            total_counts,
        );
    }

    let waypoints = timeline_waypoints(document);
    let mut waypoint_hit = false;
    let mut previous_label_right = waypoint_lane.left();
    for waypoint in waypoints
        .iter()
        .filter(|waypoint| viewport.visible_range().contains(&(waypoint.count as f32)))
    {
        let x = viewport.count_to_x(waypoint.count as f32, rect.left(), rect.width());
        let flag = [
            Pos2::new(x, waypoint_lane.top() + 3.0),
            Pos2::new(x + 8.0, waypoint_lane.top() + 7.0),
            Pos2::new(x, waypoint_lane.top() + 11.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            flag.to_vec(),
            Color32::from_rgb(132, 171, 255),
            Stroke::new(1.0, Color32::WHITE),
        ));
        let label_width = waypoint.label.chars().count() as f32 * 7.0 + 12.0;
        if x + 10.0 >= previous_label_right + 8.0 {
            painter.text(
                Pos2::new(x + 10.0, waypoint_lane.center().y),
                egui::Align2::LEFT_CENTER,
                &waypoint.label,
                egui::FontId::proportional(11.0),
                Color32::from_rgb(208, 222, 255),
            );
            previous_label_right = x + 10.0 + label_width;
        }
        let hit_rect = Rect::from_center_size(
            Pos2::new(x, waypoint_lane.center().y),
            Vec2::new(24.0, waypoint_lane.height()),
        );
        let marker_response = ui
            .interact(
                hit_rect,
                response.id.with(("waypoint", waypoint.set_index)),
                Sense::click(),
            )
            .on_hover_text(format!(
                "{} · {} {}\n{}",
                waypoint.label,
                super::i18n::registered(locale, "timeline.018"),
                waypoint.count,
                waypoint.detail
            ));
        if marker_response.clicked() {
            change.seek = Some(document.locate_count(waypoint.count as f32));
            waypoint_hit = true;
        }
    }
    for marker in document
        .production_markers
        .iter()
        .filter(|marker| viewport.visible_range().contains(&(marker.count as f32)))
    {
        let x = viewport.count_to_x(marker.count as f32, rect.left(), rect.width());
        let color = match marker.kind {
            ProductionMarkerKind::Hit => Color32::from_rgb(255, 198, 72),
            ProductionMarkerKind::Rehearsal => Color32::from_rgb(126, 216, 172),
            ProductionMarkerKind::Note => Color32::from_rgb(181, 160, 255),
        };
        painter.circle_filled(Pos2::new(x, waypoint_lane.center().y), 5.0, color);
        let marker_response = ui
            .interact(
                Rect::from_center_size(
                    Pos2::new(x, waypoint_lane.center().y),
                    Vec2::new(18.0, waypoint_lane.height()),
                ),
                response.id.with(("production-marker", marker.id.get())),
                Sense::click(),
            )
            .on_hover_text(format!(
                "{} · {} {}\n{}",
                marker_kind_name(marker.kind, locale),
                super::i18n::registered(locale, "timeline.009"),
                marker.count,
                if marker.detail.trim().is_empty() {
                    &marker.label
                } else {
                    &marker.detail
                }
            ));
        if marker_response.clicked() {
            change.seek = Some(document.locate_count(marker.count as f32));
            waypoint_hit = true;
        }
    }

    let mut start_count = 0.0;
    for (index, set) in document
        .sets
        .iter()
        .enumerate()
        .take(document.sets.len().saturating_sub(1))
    {
        let end_count = start_count + f32::from(set.counts);
        let left = viewport.count_to_x(start_count, rect.left(), rect.width());
        let right = viewport.count_to_x(end_count, rect.left(), rect.width());
        if right < rect.left() || left > rect.right() {
            start_count = end_count;
            continue;
        }
        let segment =
            Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(right, rect.bottom()));
        painter.rect_filled(
            segment,
            0.0,
            if index == current_set {
                Color32::from_rgb(48, 76, 101)
            } else if index % 2 == 0 {
                Color32::from_rgb(31, 42, 55)
            } else {
                Color32::from_rgb(37, 49, 63)
            },
        );
        painter.line_segment(
            [Pos2::new(left, rect.top()), Pos2::new(left, rect.bottom())],
            Stroke::new(2.0, Color32::from_rgb(245, 197, 66)),
        );
        painter.text(
            // The top row is reserved for IN/OUT labels. Keeping the set title
            // on its own row prevents the common count-zero overlap.
            Pos2::new(left + 6.0, rect.top() + 18.0),
            egui::Align2::LEFT_TOP,
            format!("{} · {}c", set.name, set.counts),
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
        start_count = end_count;
    }

    let range_left = viewport.count_to_x(playback_start as f32, rect.left(), rect.width());
    let range_right = viewport.count_to_x(playback_end as f32, rect.left(), rect.width());
    let playhead_x = viewport.count_to_x(global, rect.left(), rect.width());
    let labels = TimelineLabelLayout::calculate(
        rect,
        range_left,
        range_right,
        playhead_x,
        playback_start,
        playback_end,
        global,
    );
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(range_left, rect.top()),
            Pos2::new(range_right, rect.bottom()),
        ),
        0.0,
        Color32::from_rgba_unmultiplied(78, 196, 133, 30),
    );
    for track in &document.camera_program.tracks {
        for key in track
            .keyframes()
            .iter()
            .filter(|key| viewport.visible_range().contains(&key.count))
        {
            let x = viewport.count_to_x(key.count, rect.left(), rect.width());
            let center = Pos2::new(x, rect.bottom() - 27.0);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + Vec2::new(0.0, -5.0),
                    center + Vec2::new(5.0, 0.0),
                    center + Vec2::new(0.0, 5.0),
                    center + Vec2::new(-5.0, 0.0),
                ],
                Color32::from_rgb(200, 130, 255),
                Stroke::new(1.0, Color32::WHITE),
            ));
        }
    }
    for cut in document
        .camera_program
        .cuts
        .iter()
        .filter(|cut| viewport.visible_range().contains(&cut.count))
    {
        let x = viewport.count_to_x(cut.count, rect.left(), rect.width());
        painter.line_segment(
            [Pos2::new(x, rect.top() + 19.0), Pos2::new(x, rect.bottom())],
            Stroke::new(2.0, Color32::from_rgb(80, 220, 235)),
        );
        painter.text(
            Pos2::new(x + 2.0, rect.top() + 18.0),
            egui::Align2::LEFT_BOTTOM,
            super::i18n::registered(locale, "timeline.001"),
            egui::FontId::monospace(9.0),
            Color32::from_rgb(80, 220, 235),
        );
    }
    painter.line_segment(
        [
            Pos2::new(range_left, rect.top()),
            Pos2::new(range_left, rect.bottom()),
        ],
        Stroke::new(3.0, Color32::from_rgb(76, 220, 143)),
    );
    painter.line_segment(
        [
            Pos2::new(range_right, rect.top()),
            Pos2::new(range_right, rect.bottom()),
        ],
        Stroke::new(3.0, Color32::from_rgb(255, 173, 66)),
    );
    painter.text(
        labels.in_pos,
        egui::Align2::LEFT_TOP,
        format!("▶ IN {playback_start}"),
        egui::FontId::monospace(11.0),
        Color32::from_rgb(100, 235, 165),
    );
    painter.text(
        labels.out_pos,
        egui::Align2::RIGHT_TOP,
        format!("OUT {playback_end} ◀"),
        egui::FontId::monospace(11.0),
        Color32::from_rgb(255, 190, 92),
    );

    // WCAG 2.2 target-size floor in logical points. The visible rule remains
    // narrow, while the hit/accessibility target stays usable at every DPI.
    let handle_width = 24.0;
    let in_handle = Rect::from_center_size(
        Pos2::new(range_left, rect.center().y),
        Vec2::new(handle_width, rect.height()),
    );
    let out_handle = Rect::from_center_size(
        Pos2::new(range_right, rect.center().y),
        Vec2::new(handle_width, rect.height()),
    );
    let in_response = ui
        .interact(in_handle, response.id.with("range_in"), Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text(super::i18n::registered(locale, "timeline.002"));
    let out_response = ui
        .interact(out_handle, response.id.with("range_out"), Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text(super::i18n::registered(locale, "timeline.003"));
    in_response.widget_info(|| {
        egui::WidgetInfo::slider(
            true,
            playback_start as f64,
            super::i18n::registered(locale, "timeline.004"),
        )
    });
    out_response.widget_info(|| {
        egui::WidgetInfo::slider(
            true,
            playback_end as f64,
            super::i18n::registered(locale, "timeline.005"),
        )
    });
    let boundaries = || {
        document
            .sets
            .iter()
            .take(document.sets.len().saturating_sub(1))
            .scan(0_u32, |sum, set| {
                let current = *sum;
                *sum = sum.saturating_add(u32::from(set.counts));
                Some(current)
            })
            .chain(std::iter::once(document.timeline_counts()))
    };
    if in_response.dragged()
        && let Some(pointer) = in_response.interact_pointer_pos()
    {
        let start = snapped_viewport_count(
            pointer.x,
            rect.left(),
            rect.width(),
            *viewport,
            total_counts,
            boundaries(),
        )
        .min(playback_end.saturating_sub(1));
        change.range = Some((start, playback_end));
    }
    if out_response.dragged()
        && let Some(pointer) = out_response.interact_pointer_pos()
    {
        let end = snapped_viewport_count(
            pointer.x,
            rect.left(),
            rect.width(),
            *viewport,
            total_counts,
            boundaries(),
        )
        .max(playback_start + 1)
        .min(total_counts);
        change.range = Some((playback_start, end));
    }

    let tick_stride = timeline_tick_stride(viewport.span.ceil() as u32, rect.width());
    let first_tick = (viewport.start.floor() as u32).div_ceil(tick_stride) * tick_stride;
    let last_tick = (viewport.start + viewport.span).ceil().min(total) as u32;
    for count in (first_tick..=last_tick).step_by(tick_stride as usize) {
        let x = viewport.count_to_x(count as f32, rect.left(), rect.width());
        let major = count % tick_stride.saturating_mul(4) == 0;
        painter.line_segment(
            [
                Pos2::new(x, rect.bottom() - if major { 16.0 } else { 8.0 }),
                Pos2::new(x, rect.bottom()),
            ],
            Stroke::new(1.0, Color32::from_white_alpha(if major { 180 } else { 80 })),
        );
        if major {
            painter.text(
                Pos2::new(x + 2.0, rect.bottom() - 18.0),
                egui::Align2::LEFT_BOTTOM,
                count.to_string(),
                egui::FontId::monospace(10.0),
                Color32::from_gray(185),
            );
        }
    }
    painter.line_segment(
        [
            Pos2::new(playhead_x, rect.top()),
            Pos2::new(playhead_x, rect.bottom()),
        ],
        Stroke::new(3.0, Color32::from_rgb(255, 92, 92)),
    );
    painter.circle_filled(
        Pos2::new(playhead_x, rect.top() + 2.0),
        5.0,
        Color32::from_rgb(255, 92, 92),
    );
    painter.text(
        labels.now_pos,
        egui::Align2::CENTER_TOP,
        format!("NOW {global:.2}"),
        egui::FontId::monospace(10.0),
        Color32::WHITE,
    );

    let handles_active = in_response.hovered()
        || out_response.hovered()
        || in_response.dragged()
        || out_response.dragged();
    if !handles_active
        && !waypoint_hit
        && (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let target = snapped_viewport_count(
            pointer.x,
            rect.left(),
            rect.width(),
            *viewport,
            total_counts,
            boundaries(),
        );
        change.seek = Some(document.locate_count(target as f32));
    }
    let context_target = response.hover_pos().map(|pointer| {
        snapped_viewport_count(
            pointer.x,
            rect.left(),
            rect.width(),
            *viewport,
            total_counts,
            boundaries(),
        )
    });
    response.context_menu(|ui| {
        if let Some(target) = context_target {
            ui.label(format!(
                "{} {target}",
                super::i18n::registered(locale, "timeline.006")
            ));
            if ui
                .button(super::i18n::registered(locale, "timeline.007"))
                .clicked()
            {
                change.range = Some((target.min(playback_end.saturating_sub(1)), playback_end));
                ui.close();
            }
            if ui
                .button(super::i18n::registered(locale, "timeline.008"))
                .clicked()
            {
                change.range = Some((
                    playback_start,
                    target.max(playback_start + 1).min(total_counts),
                ));
                ui.close();
            }
            ui.separator();
            let existing = document
                .production_markers
                .iter()
                .find(|marker| marker.count == target);
            if let Some(marker) = existing {
                ui.label(super::i18n::registered(locale, "timeline.013"));
                // Context menus are rebuilt each frame. Keep an explicit
                // transient draft in egui memory so typing never creates a
                // history entry per character (or loses text between frames).
                let draft_id = response
                    .id
                    .with(("production-marker-draft", marker.id.get()));
                let mut edited = ui
                    .data_mut(|data| data.get_temp::<Option<ProductionMarker>>(draft_id))
                    .flatten()
                    .unwrap_or_else(|| marker.clone());
                ui.horizontal(|ui| {
                    for kind in [
                        ProductionMarkerKind::Hit,
                        ProductionMarkerKind::Rehearsal,
                        ProductionMarkerKind::Note,
                    ] {
                        if ui
                            .selectable_label(edited.kind == kind, marker_kind_name(kind, locale))
                            .clicked()
                        {
                            edited.kind = kind;
                        }
                    }
                });
                ui.label(super::i18n::registered(locale, "timeline.014"));
                ui.text_edit_singleline(&mut edited.label);
                ui.label(super::i18n::registered(locale, "timeline.015"));
                ui.add(egui::TextEdit::multiline(&mut edited.detail).desired_rows(2));
                let save = ui
                    .button(super::i18n::registered(locale, "timeline.019"))
                    .clicked();
                let cancel = ui
                    .button(super::i18n::registered(locale, "timeline.020"))
                    .clicked();
                if save {
                    if edited != *marker {
                        change.marker_edit = Some(Edit::SetProductionMarker { marker: edited });
                    }
                    ui.data_mut(|data| data.remove_temp::<Option<ProductionMarker>>(draft_id));
                    ui.close();
                } else if cancel {
                    ui.data_mut(|data| data.remove_temp::<Option<ProductionMarker>>(draft_id));
                    ui.close();
                } else {
                    ui.data_mut(|data| data.insert_temp(draft_id, Some(edited)));
                }
                if ui
                    .button(super::i18n::registered(locale, "timeline.016"))
                    .clicked()
                {
                    change.marker_edit = Some(Edit::RemoveProductionMarker { id: marker.id });
                    ui.data_mut(|data| data.remove_temp::<Option<ProductionMarker>>(draft_id));
                    ui.close();
                }
            } else if let Some(id) = next_marker_id(document) {
                ui.label(super::i18n::registered(locale, "timeline.017"));
                for kind in [
                    ProductionMarkerKind::Hit,
                    ProductionMarkerKind::Rehearsal,
                    ProductionMarkerKind::Note,
                ] {
                    if ui.button(marker_kind_name(kind, locale)).clicked() {
                        change.marker_edit = Some(Edit::InsertProductionMarker {
                            marker: ProductionMarker {
                                id,
                                count: target,
                                kind,
                                label: marker_kind_name(kind, locale).into(),
                                detail: String::new(),
                            },
                        });
                        ui.close();
                    }
                }
            }
        }
    });
    change
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_annotations_become_set_start_waypoints_only_when_named() {
        let mut document = Document::demo(1, 3);
        let mut third = document.sets[1].clone();
        third.name = "Set 3".into();
        document.sets.push(third);
        document.sets[0].counts = 8;
        document.sets[1].counts = 12;
        document.sets[1].annotation.rehearsal_mark = "B".into();
        document.sets[2].annotation.title = "Impact".into();
        document.sets[2].annotation.notes = "Hold picture".into();

        let waypoints = timeline_waypoints(&document);
        assert_eq!(waypoints.len(), 2);
        assert_eq!(waypoints[0].count, 8);
        assert_eq!(waypoints[0].label, "B");
        assert_eq!(waypoints[1].count, 20);
        assert_eq!(waypoints[1].label, "Impact");
        assert_eq!(waypoints[1].detail, "Hold picture");
    }

    #[test]
    fn production_markers_can_land_inside_a_set() {
        let mut document = Document::demo(1, 1);
        document.production_markers.push(ProductionMarker {
            id: ProductionMarkerId::new(1).unwrap(),
            count: 5,
            kind: ProductionMarkerKind::Hit,
            label: "Impact".into(),
            detail: String::new(),
        });
        assert_eq!(next_marker_id(&document), ProductionMarkerId::new(2));
        assert_eq!(
            document.locate_count(document.production_markers[0].count as f32),
            (0, 5.0)
        );
    }

    #[test]
    fn adjacent_marker_navigation_skips_the_current_marker() {
        let mut document = Document::demo(1, 1);
        for (id, count) in [(1, 4), (2, 8), (3, 12)] {
            document.production_markers.push(ProductionMarker {
                id: ProductionMarkerId::new(id).unwrap(),
                count,
                kind: ProductionMarkerKind::Hit,
                label: String::new(),
                detail: String::new(),
            });
        }
        assert_eq!(
            adjacent_production_marker_count(&document, 8, false),
            Some(4)
        );
        assert_eq!(
            adjacent_production_marker_count(&document, 8, true),
            Some(12)
        );
        assert_eq!(adjacent_production_marker_count(&document, 0, false), None);
        assert_eq!(adjacent_production_marker_count(&document, 12, true), None);
    }

    #[test]
    fn count_snaps_to_integer() {
        assert_eq!(
            snapped_viewport_count(
                53.1,
                0.0,
                100.0,
                TimelineViewport::fit(100),
                100,
                [].into_iter()
            ),
            53
        );
    }
    #[test]
    fn nearby_set_boundary_wins() {
        assert_eq!(
            snapped_viewport_count(
                47.0,
                0.0,
                100.0,
                TimelineViewport::fit(100),
                100,
                [0, 50, 100].into_iter()
            ),
            50
        );
    }
    #[test]
    fn count_is_clamped_to_timeline() {
        assert_eq!(
            snapped_viewport_count(
                200.0,
                0.0,
                100.0,
                TimelineViewport::fit(64),
                64,
                [].into_iter()
            ),
            64
        );
    }
    #[test]
    fn long_timeline_bounds_tick_count_to_viewport() {
        let stride = timeline_tick_stride(1_000_000, 1_000.0);
        assert!(stride >= 8_192);
        assert!(1_000_000_u32.div_ceil(stride) <= 123);
    }
    #[test]
    fn zoom_preserves_count_under_pointer() {
        let mut viewport = TimelineViewport::fit(1_000);
        viewport.zoom_at(0.25, 750.0, 1_000);
        assert_eq!(viewport.span, 250.0);
        assert_eq!(viewport.start, 562.5);
        assert_eq!(viewport.x_to_count(75.0, 0.0, 100.0), 750.0);
    }
    #[test]
    fn pan_and_zoom_are_clamped_to_document() {
        let mut viewport = TimelineViewport::fit(100);
        viewport.zoom_at(0.1, 50.0, 100);
        viewport.pan(1_000.0, 100);
        assert_eq!(viewport.start + viewport.span, 100.0);
        viewport.pan(-1_000.0, 100);
        assert_eq!(viewport.start, 0.0);
    }
    #[test]
    fn follow_recenters_without_changing_zoom() {
        let mut viewport = TimelineViewport {
            start: 100.0,
            span: 40.0,
        };
        assert!(!viewport.contains_with_margin(138.0, 0.08));
        // `center_on` was replaced by a glide (see `glide_start_toward`'s doc
        // comment): drive it with generous per-step dt until it reports
        // convergence, which is exactly what a long-running app does across
        // many real frames. The destination must match the old instant jump.
        let target = viewport.settled_center_start(138.0, 1_000);
        while viewport.glide_start_toward(target, 1.0, 1_000) {}
        assert_eq!(viewport.span, 40.0);
        assert_eq!(viewport.start, 118.0);
        assert!(viewport.contains_with_margin(138.0, 0.08));
    }

    #[test]
    fn glide_is_frame_rate_independent() {
        // Covering the same total time in many small steps (240Hz) or few
        // large ones (60Hz) must land on the same spot, within float noise.
        // This is the property that makes `x += (target - x) * k` wrong and
        // the `1 - e^(-dt/tau)` form correct: the former's per-frame fraction
        // compounds differently depending on step count for the same elapsed
        // time, so it would fail this exact check.
        let target = 500.0;
        let total = 1_000;

        let mut fast = TimelineViewport {
            start: 0.0,
            span: 40.0,
        };
        for _ in 0..240 {
            fast.glide_start_toward(target - fast.span * 0.5, 1.0 / 240.0, total);
        }

        let mut slow = TimelineViewport {
            start: 0.0,
            span: 40.0,
        };
        for _ in 0..60 {
            slow.glide_start_toward(target - slow.span * 0.5, 1.0 / 60.0, total);
        }

        assert!((fast.start - slow.start).abs() < 0.01);
    }
    #[test]
    fn zoomed_seek_uses_visible_counts() {
        let viewport = TimelineViewport {
            start: 400.0,
            span: 100.0,
        };
        assert_eq!(
            snapped_viewport_count(50.0, 0.0, 100.0, viewport, 1_000, [].into_iter()),
            450
        );
    }

    #[test]
    fn labels_stay_inside_and_separate_at_supported_dpi_bounds() {
        // A 1280 physical-pixel window yields these logical widths at the
        // supported 100%, 150% and 200% display scales.
        for scale in [1.0_f32, 1.5, 2.0] {
            let width = 1280.0 / scale - 16.0;
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 54.0));
            for (left, right, now) in [
                (0.0, width, 0.0),
                (width * 0.45, width * 0.55, width * 0.5),
                (width - 3.0, width, width),
            ] {
                let layout = TimelineLabelLayout::calculate(
                    rect, left, right, now, 123_456, 234_567, 123_456.75,
                );
                for position in [layout.in_pos, layout.out_pos, layout.now_pos] {
                    assert!(
                        rect.contains(position),
                        "scale={scale}, position={position:?}"
                    );
                }
                let text_width = |text: &str| text.chars().count() as f32 * 7.0 + 4.0;
                let in_width = text_width("▶ IN 123456");
                let out_width = text_width("OUT 234567 ◀");
                let now_width = text_width("NOW 123456.75");
                assert!(layout.in_pos.x + in_width <= rect.right() + f32::EPSILON);
                assert!(layout.out_pos.x - out_width >= rect.left() - f32::EPSILON);
                assert!(layout.now_pos.x - now_width * 0.5 >= rect.left() - f32::EPSILON);
                assert!(layout.now_pos.x + now_width * 0.5 <= rect.right() + f32::EPSILON);
                // Same-row anchors must retain enough horizontal separation;
                // overlapping labels are explicitly assigned different rows.
                if (layout.in_pos.y - layout.out_pos.y).abs() < 1.0 {
                    assert!(layout.out_pos.x - layout.in_pos.x >= 100.0);
                }
                if (layout.in_pos.y - layout.now_pos.y).abs() < 1.0 {
                    assert!((layout.now_pos.x - layout.in_pos.x).abs() >= 48.0);
                }
                if (layout.out_pos.y - layout.now_pos.y).abs() < 1.0 {
                    assert!((layout.out_pos.x - layout.now_pos.x).abs() >= 48.0);
                }
            }
        }
    }

    #[test]
    fn range_handles_publish_named_accesskit_nodes_at_supported_scales() {
        for scale in [1.0_f32, 2.0] {
            let context = egui::Context::default();
            context.enable_accesskit();
            context.set_pixels_per_point(scale);
            let mut viewport = TimelineViewport::fit(64);
            let output = context.run_ui(egui::RawInput::default(), |ui| {
                ui.set_width(640.0);
                let document = Document::demo(2, 2);
                let _ = draw_count_track(
                    ui,
                    &document,
                    0,
                    0.0,
                    0,
                    8,
                    &mut viewport,
                    drill_core::Locale::En,
                );
            });
            let update = output
                .platform_output
                .accesskit_update
                .expect("AccessKit tree should be generated when enabled");
            for expected in ["Playback range start (IN)", "Playback range end (OUT)"] {
                let node = update
                    .nodes
                    .iter()
                    .map(|(_, node)| node)
                    .find(|node| node.label() == Some(expected))
                    .unwrap_or_else(|| panic!("missing accessible node {expected} at {scale}x"));
                assert_eq!(node.role(), egui::accesskit::Role::Slider);
                let bounds = node.bounds().expect("interactive node needs screen bounds");
                // AccessKit bounds are expressed in egui logical points; the
                // platform adapter applies the DPI transform afterwards.
                assert!(bounds.width() >= 24.0);
                assert!(bounds.height() >= 40.0);
            }
        }
    }
}
