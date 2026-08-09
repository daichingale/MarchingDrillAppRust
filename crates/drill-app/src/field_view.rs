use super::*;

impl DrillApp {
    pub(crate) fn draw_stadium(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        rect: Rect,
    ) {
        if self.camera_program_preview
            && let Some(pose) = self.document.camera_program.evaluate(self.count_position)
        {
            self.camera = pose.orbit_camera();
        }
        if response.dragged() {
            self.camera_program_preview = false;
            let d = response.drag_delta();
            self.camera.yaw += d.x * 0.008;
            self.camera.pitch = (self.camera.pitch + d.y * 0.008).clamp(0.03, 1.54);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.camera.distance =
                    (self.camera.distance * (1.0 - scroll * 0.0015)).clamp(8.0, 2000.0);
            }
        }
        painter.rect_filled(rect, 4.0, Color32::from_rgb(16, 20, 28));
        let mut refresh_visibility = false;
        egui::Area::new("stadium-visibility-controls".into())
            .fixed_pos(rect.left_top() + Vec2::new(12.0, 34.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    refresh_visibility = self.stadium_inspector.controls(ui, self.locale);
                });
            });
        if refresh_visibility {
            self.stadium_inspector
                .analyze(&self.document, &self.frame_positions, self.camera);
        }
        let grid = &self.document.grid;
        let vw = rect.width();
        let vh = rect.height();
        let origin = rect.left_top().to_vec2();
        let proj = |world: [f32; 3]| {
            self.camera
                .project(world, vw, vh)
                .map(|[x, y]| Pos2::new(x, y) + origin)
        };
        let field = |p: Point| proj(drill_core::camera::field_to_world(p, 0.0));

        let corners = [
            Point { x: 0.0, y: 0.0 },
            Point {
                x: grid.width,
                y: 0.0,
            },
            Point {
                x: grid.width,
                y: grid.height,
            },
            Point {
                x: 0.0,
                y: grid.height,
            },
        ];
        if let Some(poly) = corners
            .iter()
            .map(|&c| field(c))
            .collect::<Option<Vec<_>>>()
        {
            painter.add(egui::Shape::convex_polygon(
                poly,
                Color32::from_rgb(25, 71, 45),
                Stroke::new(2.0, Color32::from_gray(210)),
            ));
        }
        // Raked grandstands provide a stable depth frame without pretending to
        // model a particular venue.
        for (near_side, rows) in [(true, 8_u16), (false, 5_u16)] {
            for row in 0..rows {
                let depth = 4.0 + f32::from(row) * 1.2;
                let height = 0.5 + f32::from(row) * 0.65;
                let z = if near_side {
                    -depth
                } else {
                    grid.height + depth
                };
                if let (Some(a), Some(b)) = (proj([0.0, height, z]), proj([grid.width, height, z]))
                {
                    painter.line_segment([a, b], Stroke::new(1.0, Color32::from_rgb(76, 91, 108)));
                }
            }
        }
        let mut unit = 0.0;
        while unit <= grid.width + 0.001 {
            let major = (unit / (grid.major_line_interval * 2.0)).fract().abs() < 0.001;
            if let (Some(a), Some(b)) = (
                field(Point { x: unit, y: 0.0 }),
                field(Point {
                    x: unit,
                    y: grid.height,
                }),
            ) {
                painter.line_segment(
                    [a, b],
                    Stroke::new(
                        if major { 1.5 } else { 0.5 },
                        Color32::from_white_alpha(if major { 120 } else { 45 }),
                    ),
                );
            }
            unit += grid.major_line_interval.max(0.25);
        }
        for hash in &grid.hashes {
            if let (Some(a), Some(b)) = (
                field(Point {
                    x: 0.0,
                    y: hash.position,
                }),
                field(Point {
                    x: grid.width,
                    y: hash.position,
                }),
            ) {
                painter.line_segment(
                    [a, b],
                    Stroke::new(hash.weight.clamp(0.25, 4.0), Color32::from_white_alpha(110)),
                );
            }
        }

        let gpu_stadium = self.gpu.as_ref().is_some_and(|gpu| gpu.active());
        if let Some(gpu) = self.gpu.as_ref().filter(|gpu| gpu.active()) {
            gpu.update_stadium(&self.document, &self.frame_positions, self.camera);
            painter.add(gpu.callback_stadium(rect, self.camera, self.stadium_inspector.lighting()));
        }
        let eye = self.camera.position();
        let depth = |i: usize| {
            let w = drill_core::camera::field_to_world(self.frame_positions[i], 0.0);
            let (dx, dy, dz) = (w[0] - eye[0], w[1] - eye[1], w[2] - eye[2]);
            dx * dx + dy * dy + dz * dz
        };
        let mut order: Vec<usize> = (0..self.frame_positions.len()).collect();
        order.sort_by(|&a, &b| depth(b).total_cmp(&depth(a)));
        let focal = vh / (2.0 * (self.camera.fov_y_rad * 0.5).tan());
        for i in order {
            if gpu_stadium {
                break;
            }
            let ground = drill_core::camera::field_to_world(self.frame_positions[i], 0.0);
            let Some(pos) = proj(ground) else { continue };
            let dist = depth(i).sqrt().max(0.001);
            let performer = &self.document.performers[i];
            let height_px = (performer.height_m * focal / dist).clamp(2.0, 80.0);
            let radius = (height_px * 0.22).clamp(1.5, 18.0);
            let color = performer.resolved_color(&self.document.sections);
            let color =
                drill_core::stadium::shade_color(color, self.stadium_inspector.lighting(), dist);
            painter.circle_filled(
                pos + Vec2::new(0.0, radius * 0.35),
                radius * 0.9,
                Color32::from_black_alpha(75),
            );
            let body_color = Color32::from_rgb(color[0], color[1], color[2]);
            match drill_core::stadium::choose_lod(
                height_px,
                &drill_core::stadium::LodThresholds::default(),
            ) {
                drill_core::stadium::PerformerLod::Billboard => {
                    painter.circle_filled(pos, radius, body_color);
                }
                drill_core::stadium::PerformerLod::SimpleFigure => {
                    let head = pos - Vec2::new(0.0, height_px * 0.78);
                    painter.line_segment(
                        [pos, head + Vec2::new(0.0, radius)],
                        Stroke::new(radius.max(2.0), body_color),
                    );
                    painter.circle_filled(head, radius * 0.75, body_color);
                }
                drill_core::stadium::PerformerLod::InstrumentSilhouette => {
                    let head = pos - Vec2::new(0.0, height_px * 0.82);
                    painter.circle_filled(head, radius * 0.65, body_color);
                    let body = egui::Rect::from_center_size(
                        pos - Vec2::new(0.0, height_px * 0.35),
                        Vec2::new(radius * 1.4, height_px * 0.55),
                    );
                    match performer.kind {
                        drill_core::PerformerKind::Wind => {
                            painter.add(egui::Shape::convex_polygon(
                                vec![
                                    body.left_bottom(),
                                    body.right_bottom(),
                                    Pos2::new(body.center().x, body.top()),
                                ],
                                body_color,
                                Stroke::NONE,
                            ));
                        }
                        drill_core::PerformerKind::Percussion => {
                            painter.rect_filled(body, 2.0, body_color);
                        }
                        drill_core::PerformerKind::Guard => {
                            painter.line_segment(
                                [body.left_bottom(), body.right_top()],
                                Stroke::new(radius.max(2.0), body_color),
                            );
                            painter.rect_filled(
                                egui::Rect::from_min_size(
                                    body.right_top() - Vec2::new(0.0, radius),
                                    Vec2::splat(radius * 1.3),
                                ),
                                1.0,
                                body_color,
                            );
                        }
                        drill_core::PerformerKind::Prop => {
                            painter.rect_filled(body, 0.0, body_color);
                        }
                    }
                }
            }
            if self.selected.contains(&i) {
                painter.circle_stroke(pos, radius + 3.0, Stroke::new(2.0, Color32::WHITE));
            }
            if let Some(visible) = self.stadium_inspector.visible_fraction(i)
                && visible < 0.75
            {
                let red = ((1.0 - visible) * 255.0).round() as u8;
                painter.circle_stroke(
                    pos,
                    radius + 5.0,
                    Stroke::new(2.5, Color32::from_rgb(red.max(120), 45, 45)),
                );
            }
        }
        if gpu_stadium && self.stadium_inspector.enabled {
            for (i, &point) in self.frame_positions.iter().enumerate() {
                let Some(visible) = self.stadium_inspector.visible_fraction(i) else {
                    continue;
                };
                if visible >= 0.75 {
                    continue;
                }
                let Some(pos) = proj(drill_core::camera::field_to_world(point, 0.0)) else {
                    continue;
                };
                let red = ((1.0 - visible) * 255.0).round() as u8;
                painter.circle_stroke(
                    pos,
                    7.0,
                    Stroke::new(2.5, Color32::from_rgb(red.max(120), 45, 45)),
                );
            }
        }
        painter.text(
            rect.left_top() + Vec2::new(12.0, 12.0),
            egui::Align2::LEFT_TOP,
            if self.camera_program_preview {
                "REAL VIEW · カメラショット追従中（ドラッグで解除）"
            } else {
                "REAL VIEW · 自由視点（ドラッグ回転・ホイールズーム）"
            },
            egui::FontId::proportional(12.0),
            Color32::from_white_alpha(170),
        );
    }
}
