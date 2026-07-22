use drill_core::{
    Document, GridConfig, GridLine, GridStyle, History, MoveCommand, Point, Set, Unit,
    analyze_transition, evenly_spaced_arc, evenly_spaced_line,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DrillForge")
            .with_inner_size([1280.0, 800.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "DrillForge",
        options,
        Box::new(|creation| {
            install_fonts(&creation.egui_ctx);
            Ok(Box::new(DrillApp::default()))
        }),
    )
}

fn install_fonts(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Noto Sans JP".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../../assets/NotoSansJP.ttf"
        ))),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "Noto Sans JP".into());
    }
    context.set_fonts(fonts);
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(Color32::from_gray(225));
    visuals.panel_fill = Color32::from_rgb(13, 17, 23);
    visuals.window_fill = Color32::from_rgb(20, 26, 34);
    visuals.faint_bg_color = Color32::from_rgb(28, 36, 47);
    visuals.extreme_bg_color = Color32::from_rgb(8, 11, 15);
    context.set_visuals(visuals);
    context.all_styles_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 7.0);
        style.spacing.button_padding = Vec2::new(10.0, 5.0);
    });
}

struct DrillApp {
    document: Document,
    current_set: usize,
    count_position: f32,
    playing: bool,
    speed: f32,
    tempo_bpm: f32,
    playback_start: u32,
    playback_end: u32,
    loop_playback: bool,
    last_frame: Instant,
    frame_positions: Vec<Point>,
    selected: BTreeSet<usize>,
    history: History,
    drag_before: Option<Vec<Point>>,
    drag_origin: Option<Pos2>,
    marquee_origin: Option<Pos2>,
    current_path: Option<PathBuf>,
    dirty: bool,
    status: String,
    last_autosave: Instant,
    show_guidance: bool,
}

impl Default for DrillApp {
    fn default() -> Self {
        let document = Document::demo(8, 10);
        let playback_end = document.timeline_counts();
        Self {
            frame_positions: Vec::with_capacity(document.performers.len()),
            document,
            current_set: 0,
            count_position: 0.0,
            playing: false,
            speed: 1.0,
            tempo_bpm: 120.0,
            playback_start: 0,
            playback_end,
            loop_playback: false,
            last_frame: Instant::now(),
            selected: BTreeSet::new(),
            history: History::with_limit(500),
            drag_before: None,
            drag_origin: None,
            marquee_origin: None,
            current_path: None,
            dirty: false,
            status: "準備完了".into(),
            last_autosave: Instant::now(),
            show_guidance: true,
        }
    }
}

impl DrillApp {
    fn seek_global(&mut self, count: f32) {
        let (set_index, local_count) = self.document.locate_count(count);
        self.current_set = set_index;
        self.count_position = local_count;
    }

    fn toggle_playback(&mut self, context: &egui::Context) {
        if self.playing {
            self.playing = false;
            return;
        }
        let global = self
            .document
            .global_count(self.current_set, self.count_position);
        if global < self.playback_start as f32 || global >= self.playback_end as f32 {
            self.seek_global(self.playback_start as f32);
        }
        self.playing = self.playback_end > self.playback_start;
        if self.playing {
            context.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn selected_points(&self) -> Vec<Point> {
        self.selected
            .iter()
            .map(|&i| self.document.sets[self.current_set].positions[i])
            .collect()
    }

    fn commit_layout(&mut self, points: Vec<Point>) {
        let indices = self.selected.iter().copied().collect::<Vec<_>>();
        let before = self.selected_points();
        let after = points
            .into_iter()
            .map(|point| self.document.grid.snap(point))
            .collect::<Vec<_>>();
        if before == after {
            return;
        }
        let command = MoveCommand {
            set_index: self.current_set,
            performer_indices: indices,
            before,
            after,
        };
        command.apply(&mut self.document, true);
        self.history.push(command);
        self.dirty = true;
    }

    fn selection_bounds(&self) -> Option<(Point, Point)> {
        let points = self.selected_points();
        let first = *points.first()?;
        Some(
            points
                .iter()
                .skip(1)
                .fold((first, first), |(min, max), point| {
                    (
                        Point {
                            x: min.x.min(point.x),
                            y: min.y.min(point.y),
                        },
                        Point {
                            x: max.x.max(point.x),
                            y: max.y.max(point.y),
                        },
                    )
                }),
        )
    }

    fn transform_selection(&mut self, scale: f32, angle: f32) {
        let points = self.selected_points();
        if points.is_empty() {
            return;
        }
        let center = Point {
            x: points.iter().map(|p| p.x).sum::<f32>() / points.len() as f32,
            y: points.iter().map(|p| p.y).sum::<f32>() / points.len() as f32,
        };
        let (sin, cos) = angle.sin_cos();
        let transformed = points
            .into_iter()
            .map(|point| {
                let x = (point.x - center.x) * scale;
                let y = (point.y - center.y) * scale;
                Point {
                    x: (center.x + x * cos - y * sin).clamp(0.0, self.document.grid.width),
                    y: (center.y + x * sin + y * cos).clamp(0.0, self.document.grid.height),
                }
            })
            .collect();
        self.commit_layout(transformed);
    }

    fn save_to(&mut self, path: &Path) -> Result<(), String> {
        let json = self.document.to_json().map_err(|error| error.to_string())?;
        if path.exists() {
            let backup = path.with_extension("backup.drill.json");
            std::fs::copy(path, backup).map_err(|error| error.to_string())?;
        }
        std::fs::write(path, json).map_err(|error| error.to_string())?;
        self.current_path = Some(path.to_path_buf());
        self.dirty = false;
        self.status = format!("保存しました: {}", path.display());
        Ok(())
    }

    fn save_dialog(&mut self) {
        let path = self.current_path.clone().or_else(|| {
            rfd::FileDialog::new()
                .add_filter("DrillForge", &["drill.json"])
                .set_file_name("untitled.drill.json")
                .save_file()
        });
        if let Some(path) = path
            && let Err(error) = self.save_to(&path)
        {
            self.status = format!("保存エラー: {error}");
        }
    }

    fn open_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DrillForge", &["json"])
            .pick_file()
        else {
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|json| Document::from_json(&json));
        match result {
            Ok(document) => {
                self.document = document;
                self.current_path = Some(path.clone());
                self.current_set = 0;
                self.count_position = 0.0;
                self.selected.clear();
                self.history = History::with_limit(500);
                self.playback_start = 0;
                self.playback_end = self.document.timeline_counts();
                self.dirty = false;
                self.status = format!("開きました: {}", path.display());
            }
            Err(error) => self.status = format!("読込エラー: {error}"),
        }
    }
}

impl eframe::App for DrillApp {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let visuals = ui.visuals_mut();
        visuals.override_text_color = None;
        visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(225);
        visuals.widgets.inactive.fg_stroke.color = Color32::from_rgb(22, 27, 34);
        visuals.widgets.hovered.fg_stroke.color = Color32::WHITE;
        visuals.widgets.active.fg_stroke.color = Color32::WHITE;
        visuals.widgets.open.fg_stroke.color = Color32::WHITE;
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(36, 45, 58);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(52, 66, 84);
        visuals.widgets.active.bg_fill = Color32::from_rgb(67, 88, 112);
        visuals.widgets.open.bg_fill = Color32::from_rgb(45, 58, 74);
        visuals.selection.bg_fill = Color32::from_rgb(42, 112, 163);
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        if self.dirty && self.last_autosave.elapsed().as_secs() >= 30 {
            if let Some(path) = &self.current_path
                && let Ok(json) = self.document.to_json()
            {
                let autosave = path.with_extension("autosave.drill.json");
                if std::fs::write(&autosave, json).is_ok() {
                    self.status = format!("自動保存: {}", autosave.display());
                }
            }
            self.last_autosave = Instant::now();
        }
        if self.playing {
            let global = self
                .document
                .global_count(self.current_set, self.count_position);
            let next = global + dt * (self.tempo_bpm / 60.0) * self.speed;
            if next >= self.playback_end as f32 {
                if self.loop_playback {
                    self.seek_global(self.playback_start as f32);
                } else {
                    self.seek_global(self.playback_end as f32);
                    self.playing = false;
                }
            } else {
                self.seek_global(next);
            }
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        let set_counts = self.document.sets[self.current_set].counts.max(1) as f32;
        self.document.positions_at(
            self.current_set,
            self.count_position / set_counts,
            &mut self.frame_positions,
        );
        let undo = ui.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::Z,
            ))
        });
        let redo = ui.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::Y,
            ))
        });
        let save = ui.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::S,
            ))
        });
        let toggle_playback = ui.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::NONE,
                egui::Key::Space,
            ))
        });
        if undo {
            self.history.undo(&mut self.document);
            self.dirty = true;
        }
        if redo {
            self.history.redo(&mut self.document);
            self.dirty = true;
        }
        if save {
            self.save_dialog();
        }
        if toggle_playback {
            self.toggle_playback(ui.ctx());
        }

        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("ファイル", |ui| {
                if ui.button("開く…").clicked() {
                    self.open_dialog();
                    ui.close();
                }
                if ui.button("保存    Ctrl/Cmd+S").clicked() {
                    self.save_dialog();
                    ui.close();
                }
            });
            ui.menu_button("編集", |ui| {
                if ui
                    .add_enabled(
                        self.history.can_undo(),
                        egui::Button::new("元に戻す    Ctrl/Cmd+Z"),
                    )
                    .clicked()
                {
                    self.history.undo(&mut self.document);
                    self.dirty = true;
                    ui.close();
                }
                if ui
                    .add_enabled(
                        self.history.can_redo(),
                        egui::Button::new("やり直す    Ctrl/Cmd+Y"),
                    )
                    .clicked()
                {
                    self.history.redo(&mut self.document);
                    self.dirty = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("全員を選択").clicked() {
                    self.selected = (0..self.document.performers.len()).collect();
                    ui.close();
                }
                if ui.button("選択解除").clicked() {
                    self.selected.clear();
                    ui.close();
                }
            });
            ui.menu_button("再生", |ui| {
                if ui
                    .button(if self.playing {
                        "一時停止    Space"
                    } else {
                        "再生    Space"
                    })
                    .clicked()
                {
                    self.toggle_playback(ui.ctx());
                    ui.close();
                }
                if ui.button("範囲の先頭へ").clicked() {
                    self.seek_global(self.playback_start as f32);
                    self.playing = false;
                    ui.close();
                }
                ui.separator();
                if ui.button("現在のセットを範囲にする").clicked() {
                    let start = self.document.global_count(self.current_set, 0.0) as u32;
                    self.playback_start = start;
                    self.playback_end = (start
                        + u32::from(self.document.sets[self.current_set].counts))
                    .min(self.document.timeline_counts());
                    ui.close();
                }
                if ui.button("曲全体を範囲にする").clicked() {
                    self.playback_start = 0;
                    self.playback_end = self.document.timeline_counts();
                    ui.close();
                }
                ui.checkbox(&mut self.loop_playback, "ループ再生");
            });
            ui.menu_button("表示", |ui| {
                ui.checkbox(&mut self.show_guidance, "操作ガイド");
                ui.checkbox(&mut self.document.grid.show_step_grid, "ステップグリッド");
            });
            ui.menu_button("ヘルプ", |ui| {
                ui.label("クリック: 演者を選択");
                ui.label("空白からドラッグ: 範囲選択");
                ui.label("Space: 再生／一時停止");
                ui.label("Ctrl/Cmd+S: 保存");
                ui.label("Ctrl/Cmd+Z: Undo");
            });
        });

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("DRILLFORGE")
                    .size(24.0)
                    .strong()
                    .color(Color32::from_rgb(245, 197, 66)),
            );
            ui.label(
                egui::RichText::new("Marching Design Studio")
                    .italics()
                    .color(Color32::from_gray(160)),
            );
            if ui
                .button("ファイルを開く")
                .on_hover_text("保存済みの .drill.json を開きます")
                .clicked()
            {
                self.open_dialog();
            }
            if ui
                .button("保存")
                .on_hover_text("Ctrl/Cmd+S · バックアップも自動作成します")
                .clicked()
            {
                self.save_dialog();
            }
            ui.label(if self.dirty {
                "● 未保存"
            } else {
                "✓ 保存済み"
            });
            ui.separator();
            if ui
                .button(if self.playing {
                    "⏸ 一時停止"
                } else {
                    "▶ 再生"
                })
                .clicked()
            {
                self.toggle_playback(ui.ctx());
            }
            if ui
                .button("範囲先頭")
                .on_hover_text("設定した再生範囲の開始位置へ戻ります")
                .clicked()
            {
                self.seek_global(self.playback_start as f32);
                self.playing = false;
            }
            if ui
                .add_enabled(self.history.can_undo(), egui::Button::new("↶ Undo"))
                .clicked()
            {
                self.history.undo(&mut self.document);
                self.dirty = true;
            }
            if ui
                .add_enabled(self.history.can_redo(), egui::Button::new("↷ Redo"))
                .clicked()
            {
                self.history.redo(&mut self.document);
                self.dirty = true;
            }
            ui.add(egui::Slider::new(&mut self.speed, 0.25..=4.0).text("速度"));
            ui.add(
                egui::DragValue::new(&mut self.tempo_bpm)
                    .range(20.0..=300.0)
                    .suffix(" BPM"),
            );
            ui.separator();
            ui.label(format!("演者 {}人", self.document.performers.len()));
        });
        if self.show_guidance {
            egui::Frame::new()
                .fill(Color32::from_rgb(29, 38, 51))
                .inner_margin(8)
                .corner_radius(5)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new("1  演者をクリック").strong());
                        ui.label("→");
                        ui.label(egui::RichText::new("2  Ctrl/Cmdで複数選択").strong());
                        ui.label("→");
                        ui.label(egui::RichText::new("3  ドラッグまたは配置ツール").strong());
                        ui.separator();
                        ui.label("再生中もカウント単位でシークできます");
                    });
                });
        }
        let total_counts = self.document.timeline_counts();
        egui::Frame::new()
            .fill(Color32::from_rgb(20, 27, 36))
            .inner_margin(8)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("再生範囲").strong());
                    if ui.button("現在のセット").clicked() {
                        self.playback_start =
                            self.document.global_count(self.current_set, 0.0) as u32;
                        self.playback_end = (self.playback_start
                            + u32::from(self.document.sets[self.current_set].counts))
                        .min(total_counts);
                    }
                    if ui.button("曲全体").clicked() {
                        self.playback_start = 0;
                        self.playback_end = total_counts;
                    }
                    let current_global = self
                        .document
                        .global_count(self.current_set, self.count_position)
                        .round() as u32;
                    if ui.button("現在位置を開始").clicked() {
                        self.playback_start =
                            current_global.min(self.playback_end.saturating_sub(1));
                    }
                    if ui.button("現在位置を終了").clicked() {
                        self.playback_end = current_global
                            .max(self.playback_start + 1)
                            .min(total_counts);
                    }
                    ui.checkbox(&mut self.loop_playback, "ループ");
                    ui.label(format!(
                        "COUNT {} → {}",
                        self.playback_start, self.playback_end
                    ));
                });
            });
        if let Some((set_index, local_count)) = draw_count_track(
            ui,
            &self.document,
            self.current_set,
            self.count_position,
            self.playback_start,
            self.playback_end,
        ) {
            self.current_set = set_index;
            self.count_position = local_count;
            self.playing = false;
            self.selected.clear();
        }
        ui.separator();

        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(300.0, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.heading("1. セットを選ぶ");
                    ui.small("各セットは停止位置、countsは次セットまでの拍数です");
                    for (index, set) in self.document.sets.iter().enumerate() {
                        let text = format!("{}  ·  {} counts", set.name, set.counts);
                        if ui
                            .selectable_label(index == self.current_set, text)
                            .clicked()
                        {
                            self.current_set = index;
                            self.count_position = 0.0;
                            self.playing = false;
                            self.selected.clear();
                        }
                    }
                    if ui.button("＋ セットを複製").clicked() {
                        let source = self.document.sets[self.current_set].clone();
                        let insert_at = self.current_set + 1;
                        self.document.sets.insert(
                            insert_at,
                            Set {
                                name: format!("セット {}", insert_at + 1),
                                ..source
                            },
                        );
                        self.current_set = insert_at;
                        self.count_position = 0.0;
                        self.dirty = true;
                    }
                    ui.separator();
                    ui.label(format!(
                        "COUNT  {:02} / {:02}",
                        self.count_position.round() as u16,
                        self.document.sets[self.current_set].counts
                    ));
                    let seek = ui.add(
                        egui::Slider::new(&mut self.count_position, 0.0..=set_counts)
                            .step_by(1.0)
                            .show_value(false),
                    );
                    if seek.drag_stopped() {
                        self.count_position = self.count_position.round();
                    }
                    ui.separator();
                    ui.heading("2. 演者を選ぶ");
                    ui.small("クリック／空白から囲む／Ctrl・Cmdで追加選択");
                    ui.horizontal(|ui| {
                        if ui.button("全員選択").clicked() {
                            self.selected = (0..self.document.performers.len()).collect();
                        }
                        if ui.button("選択解除").clicked() {
                            self.selected.clear();
                        }
                    });
                    if self.count_position != 0.0 {
                        ui.colored_label(
                            Color32::from_rgb(255, 184, 77),
                            "編集するにはセット境界（Count 0）へ移動してください",
                        );
                    }
                    if !self.selected.is_empty() {
                        ui.separator();
                        ui.heading(format!("3. 選択中: {}人", self.selected.len()));
                        if let Some((min, max)) = self.selection_bounds() {
                            ui.label(egui::RichText::new("フォーメーション").strong());
                            ui.horizontal_wrapped(|ui| {
                                if ui.small_button("横一列").clicked() {
                                    let y = (min.y + max.y) * 0.5;
                                    self.commit_layout(evenly_spaced_line(
                                        Point { x: min.x, y },
                                        Point { x: max.x, y },
                                        self.selected.len(),
                                    ));
                                }
                                if ui.small_button("縦一列").clicked() {
                                    let x = (min.x + max.x) * 0.5;
                                    self.commit_layout(evenly_spaced_line(
                                        Point { x, y: min.y },
                                        Point { x, y: max.y },
                                        self.selected.len(),
                                    ));
                                }
                                if ui.small_button("斜線").clicked() {
                                    self.commit_layout(evenly_spaced_line(
                                        min,
                                        max,
                                        self.selected.len(),
                                    ));
                                }
                                if ui.small_button("円弧").clicked() {
                                    let center = Point {
                                        x: (min.x + max.x) * 0.5,
                                        y: max.y,
                                    };
                                    let radius =
                                        ((max.x - min.x) * 0.5).max((max.y - min.y) * 0.5).max(2.5);
                                    self.commit_layout(evenly_spaced_arc(
                                        center,
                                        radius,
                                        std::f32::consts::PI,
                                        std::f32::consts::TAU,
                                        self.selected.len(),
                                    ));
                                }
                            });
                            ui.label(egui::RichText::new("回転・サイズ変更").strong());
                            ui.horizontal_wrapped(|ui| {
                                if ui.small_button("↶ 15°").clicked() {
                                    self.transform_selection(1.0, -15.0_f32.to_radians());
                                }
                                if ui.small_button("↷ 15°").clicked() {
                                    self.transform_selection(1.0, 15.0_f32.to_radians());
                                }
                                if ui.small_button("＋ 10%").clicked() {
                                    self.transform_selection(1.1, 0.0);
                                }
                                if ui.small_button("－ 10%").clicked() {
                                    self.transform_selection(0.9, 0.0);
                                }
                            });
                        }
                    }
                    let analysis = analyze_transition(&self.document, self.current_set, 0.75, 1.0);
                    ui.separator();
                    ui.heading("4. LIVE CLINIC");
                    ui.small("次セットへの動きをリアルタイム検査");
                    let collision_color = if analysis.collisions == 0 {
                        Color32::from_rgb(99, 210, 151)
                    } else {
                        Color32::from_rgb(255, 92, 92)
                    };
                    ui.colored_label(
                        collision_color,
                        format!("● 衝突候補: {}", analysis.collisions),
                    );
                    let stride_color = if analysis.excessive_strides == 0 {
                        Color32::from_rgb(99, 210, 151)
                    } else {
                        Color32::from_rgb(255, 184, 77)
                    };
                    ui.colored_label(
                        stride_color,
                        format!("● 過大歩幅: {}人", analysis.excessive_strides),
                    );
                    ui.separator();
                    ui.collapsing("5. グリッドデザイナー", |ui| {
                        let grid_before = self.document.grid.clone();
                        ui.label("プリセット");
                        ui.horizontal_wrapped(|ui| {
                            if ui.small_button("Football").clicked() {
                                self.document.replace_grid(GridConfig::default(), true);
                            }
                            if ui.small_button("Indoor").clicked() {
                                self.document.replace_grid(GridConfig::indoor(), true);
                            }
                            if ui.small_button("Soccer").clicked() {
                                self.document.replace_grid(GridConfig::soccer(), true);
                            }
                        });
                        let grid = &mut self.document.grid;
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut grid.unit, Unit::Yards, "yards");
                            ui.selectable_value(&mut grid.unit, Unit::Meters, "meters");
                        });
                        ui.label("フィールド寸法");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut grid.width)
                                    .range(10.0..=300.0)
                                    .suffix(" W"),
                            );
                            ui.add(
                                egui::DragValue::new(&mut grid.height)
                                    .range(10.0..=300.0)
                                    .suffix(" H"),
                            );
                        });
                        ui.label("左右: steps / units");
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut grid.horizontal_steps).range(1..=32));
                            ui.add(
                                egui::DragValue::new(&mut grid.horizontal_units).range(0.5..=20.0),
                            );
                        });
                        ui.label("上下: steps / units");
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut grid.vertical_steps).range(1..=32));
                            ui.add(
                                egui::DragValue::new(&mut grid.vertical_units).range(0.5..=20.0),
                            );
                        });
                        ui.add(
                            egui::Slider::new(&mut grid.major_line_interval, 1.0..=20.0)
                                .text("主線間隔"),
                        );
                        ui.add(egui::Slider::new(&mut grid.resolution, 1..=8).text("分割"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut grid.style, GridStyle::Lines, "線");
                            ui.selectable_value(&mut grid.style, GridStyle::Dots, "点");
                        });
                        ui.checkbox(&mut grid.show_step_grid, "ステップグリッド表示");
                        ui.checkbox(&mut grid.snap_enabled, "グリッドへスナップ");
                        ui.label("ハッシュ位置");
                        let grid_height = grid.height;
                        let mut remove_hash = None;
                        for (index, hash) in grid.hashes.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut hash.label);
                                ui.add(
                                    egui::DragValue::new(&mut hash.position)
                                        .range(0.0..=grid_height),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut hash.weight)
                                        .range(0.25..=4.0)
                                        .speed(0.1),
                                );
                                if ui.small_button("×").clicked() {
                                    remove_hash = Some(index);
                                }
                            });
                        }
                        if let Some(index) = remove_hash {
                            grid.hashes.remove(index);
                        }
                        if ui.small_button("＋ ハッシュ/区切り線").clicked() {
                            grid.hashes.push(GridLine {
                                position: grid.height / 2.0,
                                label: "新しい線".into(),
                                weight: 1.0,
                            });
                        }
                        if self.document.grid != grid_before {
                            self.dirty = true;
                        }
                    });
                    ui.separator();
                    ui.small(&self.status);
                },
            );
            ui.separator();
            let available = ui.available_size();
            let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
            let rect = response.rect.shrink(18.0);
            draw_field(&painter, rect, &self.document.grid);
            if self.selected.is_empty() {
                let card = Rect::from_min_size(
                    rect.left_top() + Vec2::new(18.0, 42.0),
                    Vec2::new(310.0, 76.0),
                );
                painter.rect_filled(card, 8.0, Color32::from_black_alpha(190));
                painter.rect_stroke(
                    card,
                    8.0,
                    Stroke::new(1.0, Color32::from_rgb(245, 197, 66)),
                    StrokeKind::Inside,
                );
                painter.text(
                    card.left_top() + Vec2::new(14.0, 12.0),
                    egui::Align2::LEFT_TOP,
                    "編集を始めましょう",
                    egui::FontId::proportional(18.0),
                    Color32::WHITE,
                );
                painter.text(
                    card.left_top() + Vec2::new(14.0, 42.0),
                    egui::Align2::LEFT_TOP,
                    "黄色い演者をクリック → ドラッグで移動",
                    egui::FontId::proportional(13.0),
                    Color32::from_gray(205),
                );
            }
            let to_screen = |point: Point| {
                Pos2::new(
                    rect.left() + point.x / self.document.grid.width * rect.width(),
                    rect.top() + point.y / self.document.grid.height * rect.height(),
                )
            };
            for (index, (&point, performer)) in self
                .frame_positions
                .iter()
                .zip(&self.document.performers)
                .enumerate()
            {
                let pos = to_screen(point);
                let selected = self.selected.contains(&index);
                painter.circle_filled(
                    pos,
                    if selected { 8.0 } else { 6.0 },
                    Color32::from_rgb(performer.color[0], performer.color[1], performer.color[2]),
                );
                if selected {
                    painter.circle_stroke(pos, 11.0, Stroke::new(2.0, Color32::WHITE));
                }
                painter.text(
                    pos + Vec2::new(0.0, 10.0),
                    egui::Align2::CENTER_TOP,
                    &performer.label,
                    egui::FontId::monospace(9.0),
                    Color32::WHITE,
                );
            }
            if let Some(pointer) = response.interact_pointer_pos() {
                let nearest = || {
                    self.frame_positions
                        .iter()
                        .enumerate()
                        .min_by(|(_, a), (_, b)| {
                            to_screen(**a)
                                .distance(pointer)
                                .total_cmp(&to_screen(**b).distance(pointer))
                        })
                        .filter(|(_, p)| to_screen(**p).distance(pointer) < 18.0)
                        .map(|(i, _)| i)
                };
                if response.clicked() {
                    if let Some(index) = nearest() {
                        let additive =
                            ui.input(|input| input.modifiers.command || input.modifiers.ctrl);
                        if additive {
                            if !self.selected.insert(index) {
                                self.selected.remove(&index);
                            }
                        } else {
                            self.selected.clear();
                            self.selected.insert(index);
                        }
                    } else {
                        self.selected.clear();
                    }
                }
                if response.drag_started()
                    && self.count_position == 0.0
                    && let Some(index) = nearest()
                {
                    if !self.selected.contains(&index) {
                        self.selected.clear();
                        self.selected.insert(index);
                    }
                    self.drag_before = Some(
                        self.selected
                            .iter()
                            .map(|&i| self.document.sets[self.current_set].positions[i])
                            .collect(),
                    );
                    self.drag_origin = Some(pointer);
                }
                if response.drag_started() && nearest().is_none() {
                    self.marquee_origin = Some(pointer);
                }
                if response.dragged()
                    && let Some(origin) = self.marquee_origin
                {
                    let marquee = Rect::from_two_pos(origin, pointer).intersect(rect);
                    painter.rect_filled(
                        marquee,
                        0.0,
                        Color32::from_rgba_unmultiplied(70, 160, 255, 35),
                    );
                    painter.rect_stroke(
                        marquee,
                        0.0,
                        Stroke::new(1.5, Color32::from_rgb(95, 180, 255)),
                        StrokeKind::Inside,
                    );
                }
                if response.dragged()
                    && self.count_position == 0.0
                    && let (Some(before), Some(origin)) = (&self.drag_before, self.drag_origin)
                {
                    let dx = (pointer.x - origin.x) / rect.width() * self.document.grid.width;
                    let dy = (pointer.y - origin.y) / rect.height() * self.document.grid.height;
                    for (&index, &start) in self.selected.iter().zip(before) {
                        let point = Point {
                            x: (start.x + dx).clamp(0.0, self.document.grid.width),
                            y: (start.y + dy).clamp(0.0, self.document.grid.height),
                        };
                        self.document.sets[self.current_set].positions[index] =
                            self.document.grid.snap(point);
                    }
                }
                if response.drag_stopped()
                    && let Some(before) = self.drag_before.take()
                {
                    let indices = self.selected.iter().copied().collect::<Vec<_>>();
                    let after = indices
                        .iter()
                        .map(|&i| self.document.sets[self.current_set].positions[i])
                        .collect::<Vec<_>>();
                    if before != after {
                        self.history.push(MoveCommand {
                            set_index: self.current_set,
                            performer_indices: indices,
                            before,
                            after,
                        });
                        self.dirty = true;
                    }
                    self.drag_origin = None;
                }
                if response.drag_stopped()
                    && let Some(origin) = self.marquee_origin.take()
                {
                    let marquee = Rect::from_two_pos(origin, pointer);
                    let additive =
                        ui.input(|input| input.modifiers.command || input.modifiers.ctrl);
                    if !additive {
                        self.selected.clear();
                    }
                    for (index, &point) in self.frame_positions.iter().enumerate() {
                        if marquee.contains(to_screen(point)) {
                            self.selected.insert(index);
                        }
                    }
                }
            }
        });
    }
}

fn draw_field(painter: &egui::Painter, rect: Rect, grid: &GridConfig) {
    painter.rect_filled(rect, 4.0, Color32::from_rgb(25, 71, 45));
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(2.0, Color32::from_gray(210)),
        StrokeKind::Inside,
    );
    let to_x = |x: f32| rect.left() + x / grid.width * rect.width();
    let to_y = |y: f32| rect.top() + y / grid.height * rect.height();
    let mut unit = 0.0;
    while unit <= grid.width + 0.001 {
        let x = to_x(unit);
        let major = (unit / (grid.major_line_interval * 2.0)).fract().abs() < 0.001;
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(
                if major { 1.2 } else { 0.5 },
                Color32::from_white_alpha(if major { 120 } else { 55 }),
            ),
        );
        if major && unit > 0.0 && unit < grid.width {
            painter.text(
                Pos2::new(x, rect.top() + 8.0),
                egui::Align2::CENTER_TOP,
                format!("{unit:.0}"),
                egui::FontId::proportional(13.0),
                Color32::from_white_alpha(170),
            );
        }
        unit += grid.major_line_interval.max(0.25);
    }

    if grid.show_step_grid {
        let dx = grid.horizontal_units / grid.horizontal_steps.max(1) as f32;
        let dy = grid.vertical_units / grid.vertical_steps.max(1) as f32;
        match grid.style {
            GridStyle::Lines => {
                let mut x = dx;
                while x < grid.width {
                    painter.line_segment(
                        [
                            Pos2::new(to_x(x), rect.top()),
                            Pos2::new(to_x(x), rect.bottom()),
                        ],
                        Stroke::new(0.35, Color32::from_white_alpha(28)),
                    );
                    x += dx;
                }
                let mut y = dy;
                while y < grid.height {
                    painter.line_segment(
                        [
                            Pos2::new(rect.left(), to_y(y)),
                            Pos2::new(rect.right(), to_y(y)),
                        ],
                        Stroke::new(0.35, Color32::from_white_alpha(28)),
                    );
                    y += dy;
                }
            }
            GridStyle::Dots => {
                let mut y = dy;
                while y < grid.height {
                    let mut x = dx;
                    while x < grid.width {
                        painter.circle_filled(
                            Pos2::new(to_x(x), to_y(y)),
                            0.8,
                            Color32::from_white_alpha(55),
                        );
                        x += dx;
                    }
                    y += dy;
                }
            }
        }
    }

    for hash in &grid.hashes {
        let y = to_y(hash.position.clamp(0.0, grid.height));
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(hash.weight.clamp(0.25, 4.0), Color32::from_white_alpha(120)),
        );
        painter.text(
            Pos2::new(rect.left() + 5.0, y - 3.0),
            egui::Align2::LEFT_BOTTOM,
            &hash.label,
            egui::FontId::proportional(10.0),
            Color32::from_white_alpha(150),
        );
    }
}

fn draw_count_track(
    ui: &mut egui::Ui,
    document: &Document,
    current_set: usize,
    local_count: f32,
    playback_start: u32,
    playback_end: u32,
) -> Option<(usize, f32)> {
    let total = document.timeline_counts().max(1) as f32;
    let global = document
        .global_count(current_set, local_count)
        .clamp(0.0, total);
    let (response, painter) = ui.allocate_painter(
        Vec2::new(ui.available_width(), 64.0),
        Sense::click_and_drag(),
    );
    let rect = response.rect.shrink2(Vec2::new(8.0, 5.0));
    painter.rect_filled(rect, 5.0, Color32::from_rgb(18, 23, 31));

    let mut start_count = 0.0;
    for (index, set) in document
        .sets
        .iter()
        .enumerate()
        .take(document.sets.len().saturating_sub(1))
    {
        let end_count = start_count + f32::from(set.counts);
        let left = rect.left() + start_count / total * rect.width();
        let right = rect.left() + end_count / total * rect.width();
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
            Pos2::new(left + 6.0, rect.top() + 5.0),
            egui::Align2::LEFT_TOP,
            format!("{} · {}c", set.name, set.counts),
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
        start_count = end_count;
    }

    let range_left = rect.left() + playback_start as f32 / total * rect.width();
    let range_right = rect.left() + playback_end as f32 / total * rect.width();
    let range_rect = Rect::from_min_max(
        Pos2::new(range_left, rect.top()),
        Pos2::new(range_right, rect.bottom()),
    );
    painter.rect_filled(
        range_rect,
        0.0,
        Color32::from_rgba_unmultiplied(78, 196, 133, 30),
    );
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

    for count in 0..=document.timeline_counts() {
        let x = rect.left() + count as f32 / total * rect.width();
        let major = count % 4 == 0;
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
    let playhead_x = rect.left() + global / total * rect.width();
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

    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let target = ((pointer.x - rect.left()) / rect.width() * total)
            .round()
            .clamp(0.0, total);
        return Some(document.locate_count(target));
    }
    None
}
