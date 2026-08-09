#[path = "app_ui.rs"]
mod app_ui;
#[path = "audio_state.rs"]
mod audio_state;
#[path = "bootstrap.rs"]
mod bootstrap;
#[path = "commands.rs"]
mod commands;
#[path = "controller.rs"]
mod controller;
#[path = "egui_backend.rs"]
mod egui_backend;
#[path = "export_state.rs"]
mod export_state;
#[path = "field_view.rs"]
mod field_view;
#[path = "gpu_bridge.rs"]
mod gpu_bridge;
#[path = "i18n.rs"]
mod i18n;
#[path = "import_state.rs"]
mod import_state;
#[path = "inspector_media.rs"]
mod inspector_media;
#[path = "legal_notices.rs"]
mod legal_notices;
#[path = "onboarding.rs"]
mod onboarding;
#[path = "plugin_state.rs"]
mod plugin_state;
#[path = "print_state.rs"]
mod print_state;
#[path = "project_state.rs"]
mod project_state;
#[path = "section_manager.rs"]
mod section_manager;
#[path = "stadium_inspector.rs"]
mod stadium_inspector;
#[path = "subset_snapshot_state.rs"]
mod subset_snapshot_state;
#[path = "text_export_state.rs"]
mod text_export_state;
#[path = "timeline.rs"]
mod timeline;
#[cfg(test)]
#[path = "ui_qa.rs"]
mod ui_qa;
#[path = "underlay_state.rs"]
mod underlay_state;
#[path = "update_state.rs"]
mod update_state;
#[path = "workspace_inspector.rs"]
mod workspace_inspector;

use commands::{Command as UiCommand, Menu as CommandMenu};
use drill_core::Locale;
use drill_core::route_suggestions::{
    RouteSuggestion, SuggestionConstraints, SuggestionLimits, SuggestionReason, suggest_routes,
};
use drill_core::video::{ExportPreset, VideoExportConfig};
use drill_core::{
    Document, Edit, GridConfig, GridLine, GridStyle, History, Point, Set, Unit, analyze_transition,
    camera::Camera, continuity, coordinates, editing, evenly_spaced_arc, evenly_spaced_line,
    pathing, shapes,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use i18n::{Text, text};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use timeline::{TimelineViewport, draw_count_track};

#[inline]
pub(crate) fn tr(locale: Locale, japanese: &'static str, english: &'static str) -> &'static str {
    match locale {
        Locale::Ja => japanese,
        Locale::En => english,
    }
}

pub(crate) fn run() -> eframe::Result {
    bootstrap::run()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewMode {
    Field2D,
    Stadium3D,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceFocus {
    Performer,
    Clinic,
    Grid,
    Tempo,
    Video,
    Audio,
}

pub(crate) struct DrillApp {
    document: Document,
    view_mode: ViewMode,
    camera: Camera,
    camera_program_preview: bool,
    beats_per_measure: u16,
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
    audio_state: audio_state::AudioState,
    click_settings: drill_audio::ClickSettings,
    display_list: drill_render::DisplayList,
    render_scratch: drill_render::BuildScratch,
    selected: BTreeSet<usize>,
    history: History,
    drag_before: Option<Vec<Point>>,
    drag_preview: Option<Vec<Point>>,
    drag_origin: Option<Pos2>,
    marquee_origin: Option<Pos2>,
    current_path: Option<PathBuf>,
    dirty: bool,
    status: String,
    last_autosave: Instant,
    show_guidance: bool,
    video_export: VideoExportConfig,
    video_preset: ExportPreset,
    video_advanced: bool,
    export_state: export_state::ExportState,
    project_state: project_state::ProjectState,
    embed_audio_in_project: bool,
    project_warnings: Vec<drill_project::container::LoadWarning>,
    section_manager: section_manager::SectionManager,
    timeline_view: TimelineViewport,
    timeline_follow: bool,
    onboarding: onboarding::OnboardingState,
    ever_played: bool,
    locale: Locale,
    crash_notice_dismissed: bool,
    print_state: print_state::PrintState,
    workspace_focus: Option<WorkspaceFocus>,
    show_legal_notices: bool,
    update_state: update_state::UpdateState,
    import_state: import_state::ImportState,
    gpu: Option<gpu_bridge::Bridge>,
    plugin_state: plugin_state::PluginUiState,
    text_export_state: text_export_state::TextExportState,
    subset_snapshot_state: subset_snapshot_state::SubsetSnapshotState,
    route_suggestions: Vec<RouteSuggestion>,
    route_suggestion_selected: usize,
    grid_draft: Option<GridConfig>,
    grid_draft_dirty: bool,
    tempo_draft: Option<drill_core::tempo::TempoMap>,
    tempo_draft_dirty: bool,
    audio_draft: Option<drill_core::audio::AudioTrack>,
    audio_draft_dirty: bool,
    stadium_inspector: stadium_inspector::StadiumInspector,
    formation_preview_spec: Option<shapes::ShapeSpec>,
    formation_preview_points: Vec<Point>,
    free_draw_active: bool,
    free_draw_raw: Vec<Point>,
    formation_text: String,
    underlay_state: underlay_state::UnderlayState,
}

impl Default for DrillApp {
    fn default() -> Self {
        let document = Document::demo(8, 10);
        let playback_end = document.timeline_counts();
        let camera = Camera::press_box(&document.grid);
        Self {
            frame_positions: Vec::with_capacity(document.performers.len()),
            audio_state: audio_state::AudioState::default(),
            click_settings: drill_audio::ClickSettings::default(),
            display_list: drill_render::DisplayList::new(),
            render_scratch: drill_render::BuildScratch,
            view_mode: ViewMode::Field2D,
            camera,
            camera_program_preview: true,
            beats_per_measure: 4,
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
            drag_preview: None,
            drag_origin: None,
            marquee_origin: None,
            current_path: None,
            dirty: false,
            status: text(Locale::Ja, Text::Ready).into(),
            last_autosave: Instant::now(),
            show_guidance: true,
            video_export: VideoExportConfig::default(),
            video_preset: ExportPreset::Standard,
            video_advanced: false,
            export_state: export_state::ExportState::default(),
            project_state: project_state::ProjectState::new(),
            embed_audio_in_project: true,
            project_warnings: Vec::new(),
            section_manager: section_manager::SectionManager::default(),
            timeline_view: TimelineViewport::fit(playback_end),
            timeline_follow: true,
            onboarding: onboarding::OnboardingState::default(),
            ever_played: false,
            locale: Locale::Ja,
            crash_notice_dismissed: false,
            print_state: print_state::PrintState::default(),
            workspace_focus: None,
            show_legal_notices: false,
            update_state: update_state::UpdateState::default(),
            import_state: import_state::ImportState::default(),
            gpu: None,
            plugin_state: plugin_state::PluginUiState::default(),
            text_export_state: text_export_state::TextExportState::default(),
            subset_snapshot_state: subset_snapshot_state::SubsetSnapshotState::default(),
            route_suggestions: Vec::new(),
            route_suggestion_selected: 0,
            grid_draft: None,
            grid_draft_dirty: false,
            tempo_draft: None,
            tempo_draft_dirty: false,
            audio_draft: None,
            audio_draft_dirty: false,
            stadium_inspector: stadium_inspector::StadiumInspector::default(),
            formation_preview_spec: None,
            formation_preview_points: Vec::new(),
            free_draw_active: false,
            free_draw_raw: Vec::with_capacity(512),
            formation_text: "DRILL".into(),
            underlay_state: underlay_state::UnderlayState::default(),
        }
    }
}

impl DrillApp {
    fn show_update_notice(&mut self, context: &egui::Context) {
        let Some(release) = self.update_state.available.clone() else {
            return;
        };
        let mut open = true;
        egui::Window::new(i18n::registered(self.locale, "app-state.001"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(context, |ui| {
                ui.heading(format!("DrillForge {}", release.version));
                ui.label(if self.locale == Locale::Ja {
                    &release.release_notes_ja
                } else {
                    &release.release_notes_en
                });
                ui.small(i18n::registered(self.locale, "app-state.002"));
                ui.horizontal(|ui| {
                    if ui
                        .button(i18n::registered(self.locale, "app-state.003"))
                        .on_hover_text(i18n::registered(self.locale, "app-state.004"))
                        .clicked()
                    {
                        context.copy_text(release.artifact.download_url.clone());
                        self.update_state.status =
                            update_state::UpdateStatus::DownloadAddressCopied;
                        self.update_state.available = None;
                    }
                    if ui
                        .button(i18n::registered(self.locale, "app-state.005"))
                        .clicked()
                    {
                        self.update_state.available = None;
                    }
                    if ui
                        .button(i18n::registered(self.locale, "app-state.006"))
                        .clicked()
                    {
                        self.update_state.skip_available();
                    }
                });
            });
        if !open {
            self.update_state.available = None;
        }
    }

    fn command_context(&self) -> commands::Context {
        commands::Context {
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            has_performers: !self.document.performers.is_empty(),
            has_selection: !self.selected.is_empty(),
            has_sets: !self.document.sets.is_empty(),
        }
    }

    fn command_menu(&mut self, ui: &mut egui::Ui, menu: CommandMenu) {
        if let Some(command) = commands::show_menu(ui, menu, self.command_context(), self.locale) {
            self.execute_command(command, ui.ctx());
        }
    }

    fn duplicate_current_set(&mut self) {
        let Some(source) = self.document.sets.get(self.current_set).cloned() else {
            return;
        };
        let insert_at = self.current_set + 1;
        let new_id = self
            .document
            .sets
            .iter()
            .map(|set| set.id.get())
            .max()
            .and_then(|id| id.checked_add(1))
            .and_then(drill_core::SetId::new)
            .unwrap_or(source.id);
        if new_id == source.id {
            self.status = i18n::registered(self.locale, "app-state.136").into();
            return;
        }
        let mut next = self.document.clone();
        next.sets.insert(
            insert_at,
            Set {
                id: new_id,
                name: format!("セット {}", insert_at + 1),
                ..source
            },
        );
        if let Err(error) = self.history.execute(
            &mut self.document,
            Edit::ReplaceDocument {
                document: Box::new(next),
            },
        ) {
            self.status = error.message(self.locale);
            return;
        }
        self.current_set = insert_at;
        self.count_position = 0.0;
        self.dirty = true;
    }

    fn execute_command(&mut self, command: UiCommand, context: &egui::Context) {
        match command {
            UiCommand::Undo => {
                self.history.undo(&mut self.document);
                self.dirty = true;
            }
            UiCommand::Redo => {
                self.history.redo(&mut self.document);
                self.dirty = true;
            }
            UiCommand::SelectAll => self.selected = (0..self.document.performers.len()).collect(),
            UiCommand::ClearSelection => self.selected.clear(),
            UiCommand::DuplicateSet => self.duplicate_current_set(),
            UiCommand::ManageSections => self.section_manager.open = true,
            UiCommand::PlayPause => self.toggle_playback(context),
            UiCommand::RangeStart => {
                self.seek_global(self.playback_start as f32);
                self.playing = false;
            }
            UiCommand::RangeCurrentSet => {
                let start = self.document.global_count(self.current_set, 0.0) as u32;
                self.playback_start = start;
                self.playback_end = (start
                    + u32::from(self.document.sets[self.current_set].counts))
                .min(self.document.timeline_counts());
            }
            UiCommand::RangeWholeShow => {
                self.playback_start = 0;
                self.playback_end = self.document.timeline_counts();
            }
            UiCommand::FocusPerformerTools => {
                self.workspace_focus = Some(WorkspaceFocus::Performer)
            }
            UiCommand::FocusClinic => self.workspace_focus = Some(WorkspaceFocus::Clinic),
            UiCommand::FocusGrid => self.workspace_focus = Some(WorkspaceFocus::Grid),
            UiCommand::FocusTempo => self.workspace_focus = Some(WorkspaceFocus::Tempo),
            UiCommand::FocusVideo => self.workspace_focus = Some(WorkspaceFocus::Video),
            UiCommand::OpenPrint => self.print_state.open = true,
            UiCommand::FocusAudio => self.workspace_focus = Some(WorkspaceFocus::Audio),
            UiCommand::View2d => self.view_mode = ViewMode::Field2D,
            UiCommand::View3d => self.view_mode = ViewMode::Stadium3D,
            UiCommand::ToggleGuidance => self.show_guidance = !self.show_guidance,
            UiCommand::GettingStarted => self.onboarding.show_help = true,
            UiCommand::LegalNotices => self.show_legal_notices = true,
        }
    }

    fn show_print_workspace(&mut self, context: &egui::Context) {
        self.print_state.poll(self.locale);
        if !self.print_state.open {
            return;
        }
        let selected = self.selected.iter().copied().collect::<Vec<_>>();
        let mut open = self.print_state.open;
        let mut notation = self.document.grid.coordinate_notation.clone();
        let mut notation_changed = false;
        egui::Window::new(i18n::registered(self.locale, "app-state.007"))
            .open(&mut open).default_width(720.0).resizable(true).scroll(true).show(context, |ui| {
                ui.heading(i18n::registered(self.locale, "app-state.008"));
                ui.label(i18n::registered(self.locale, "app-state.009"));
                ui.separator();
                ui.columns(2, |columns| {
                    let ui=&mut columns[0]; ui.strong(i18n::registered(self.locale, "app-state.010"));
                    ui.radio_value(&mut self.print_state.kind, drill_export::report::ReportKind::SetChart, i18n::registered(self.locale, "app-state.011"));
                    ui.radio_value(&mut self.print_state.kind, drill_export::report::ReportKind::PerformerDrillBook, i18n::registered(self.locale, "app-state.012"));
                    if self.print_state.kind == drill_export::report::ReportKind::PerformerDrillBook { ui.small(if self.locale == Locale::Ja { if selected.is_empty() { "全演者を出力します" } else { "選択中の演者だけを出力します" } } else if selected.is_empty() { "Exports every performer" } else { "Exports selected performers only" }); }
                    ui.radio_value(&mut self.print_state.kind, drill_export::report::ReportKind::CountSheet, i18n::registered(self.locale, "app-state.013"));
                    ui.radio_value(&mut self.print_state.kind, drill_export::report::ReportKind::ProductionSheet, i18n::registered(self.locale, "app-state.014"));
                    ui.add_space(10.0); ui.strong(i18n::registered(self.locale, "app-state.015"));
                    ui.horizontal(|ui| {
                        if ui.button(i18n::registered(self.locale, "app-state.016")).clicked() { notation=Default::default(); notation_changed=true; }
                        if ui.button("DCI 8-to-5").clicked() { notation=drill_core::coordinates::CoordinateNotation::dci(); notation_changed=true; }
                        if ui.button(i18n::registered(self.locale, "app-state.017")).clicked() { notation=drill_core::coordinates::CoordinateNotation::indoor(); notation_changed=true; }
                    });
                    egui::ComboBox::from_id_salt("coordinate-rounding").selected_text(format!("{:?}",notation.rounding)).show_ui(ui,|ui| {
                        use drill_core::coordinates::StepRounding::*;
                        notation_changed |= ui.selectable_value(&mut notation.rounding,Eighth,"1/8 step").changed();
                        notation_changed |= ui.selectable_value(&mut notation.rounding,Quarter,"1/4 step").changed();
                        notation_changed |= ui.selectable_value(&mut notation.rounding,Half,"1/2 step").changed();
                        notation_changed |= ui.selectable_value(&mut notation.rounding,Whole,"1 step").changed();
                    });
                    egui::ComboBox::from_id_salt("yard-line-interval").selected_text(format!("{:?}",notation.yard_lines)).show_ui(ui,|ui| {
                        use drill_core::coordinates::YardLineInterval::*;
                        notation_changed |= ui.selectable_value(&mut notation.yard_lines,Grid,i18n::registered(self.locale, "app-state.018")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.yard_lines,Five,"5 yd").changed();
                        notation_changed |= ui.selectable_value(&mut notation.yard_lines,Ten,"10 yd").changed();
                        let custom = match notation.yard_lines { Custom(v)=>v, _=>5.0 };
                        notation_changed |= ui.selectable_value(&mut notation.yard_lines,Custom(custom),i18n::registered(self.locale, "app-state.019")).changed();
                    });
                    if let drill_core::coordinates::YardLineInterval::Custom(value)=&mut notation.yard_lines { ui.horizontal(|ui| { ui.label(i18n::registered(self.locale, "app-state.020")); notation_changed |= ui.add(egui::DragValue::new(value).range(1.0..=50.0).speed(0.25)).changed(); }); }
                    ui.horizontal(|ui| {
                        use drill_core::coordinates::OnLineStyle::*;
                        ui.label(i18n::registered(self.locale, "app-state.021"));
                        notation_changed |= ui.selectable_value(&mut notation.on_line,Explicit,i18n::registered(self.locale, "app-state.022")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.on_line,Short,i18n::registered(self.locale, "app-state.023")).changed();
                    });
                    egui::ComboBox::from_id_salt("step-notation-style").selected_text(format!("{:?}",notation.step_style)).show_ui(ui,|ui| {
                        use drill_core::coordinates::StepNotationStyle::*;
                        notation_changed |= ui.selectable_value(&mut notation.step_style,Steps,i18n::registered(self.locale, "app-state.024")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.step_style,EightToFive,"8-to-5").changed();
                        notation_changed |= ui.selectable_value(&mut notation.step_style,SixToFive,"6-to-5").changed();
                    });
                    egui::ComboBox::from_id_salt("depth-reference").selected_text(match &notation.front_back { drill_core::coordinates::FrontBackReference::NearestLine=>i18n::registered(self.locale, "app-state.026"), drill_core::coordinates::FrontBackReference::NearestHash=>i18n::registered(self.locale, "app-state.025"), drill_core::coordinates::FrontBackReference::FixedLabel(v)=>v.as_str() }).show_ui(ui,|ui| {
                        use drill_core::coordinates::FrontBackReference::*;
                        notation_changed |= ui.selectable_value(&mut notation.front_back,NearestLine,i18n::registered(self.locale, "app-state.027")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.front_back,NearestHash,i18n::registered(self.locale, "app-state.028")).changed();
                        for line in &self.document.grid.hashes { notation_changed |= ui.selectable_value(&mut notation.front_back,FixedLabel(line.label.clone()),&line.label).changed(); }
                    });
                    egui::ComboBox::from_id_salt("production-template").selected_text(format!("{:?}",notation.production_template)).show_ui(ui,|ui| {
                        use drill_core::coordinates::ProductionTemplatePreset::*;
                        notation_changed |= ui.selectable_value(&mut notation.production_template,Standard,i18n::registered(self.locale, "app-state.029")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.production_template,Compact,i18n::registered(self.locale, "app-state.030")).changed();
                        notation_changed |= ui.selectable_value(&mut notation.production_template,Rehearsal,i18n::registered(self.locale, "app-state.031")).changed();
                    });
                    ui.add_space(10.0); ui.strong(i18n::registered(self.locale, "app-state.032"));
                    egui::ComboBox::from_id_salt("print-page-size").selected_text(self.print_state.page_size_label()).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.print_state.settings.page_size, drill_export::page::PageSize::A4, "A4");
                        ui.selectable_value(&mut self.print_state.settings.page_size, drill_export::page::PageSize::Letter, "Letter");
                        ui.selectable_value(&mut self.print_state.settings.page_size, drill_export::page::PageSize::Tabloid, "Tabloid");
                    });
                    egui::ComboBox::from_id_salt("print-orientation").selected_text(self.print_state.orientation_label(self.locale)).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.print_state.settings.orientation, drill_export::page::Orientation::Portrait, i18n::registered(self.locale, "app-state.033"));
                        ui.selectable_value(&mut self.print_state.settings.orientation, drill_export::page::Orientation::Landscape, i18n::registered(self.locale, "app-state.034"));
                    });
                    ui.label(i18n::registered(self.locale, "app-state.137"));
                    ui.horizontal(|ui| { ui.label(i18n::registered(self.locale, "app-state.138")); ui.add(egui::DragValue::new(&mut self.print_state.settings.margins.top_mm).range(3.0..=40.0)); ui.label(i18n::registered(self.locale, "app-state.139")); ui.add(egui::DragValue::new(&mut self.print_state.settings.margins.bottom_mm).range(3.0..=40.0)); });
                    ui.horizontal(|ui| { ui.label(i18n::registered(self.locale, "app-state.140")); ui.add(egui::DragValue::new(&mut self.print_state.settings.margins.left_mm).range(3.0..=40.0)); ui.label(i18n::registered(self.locale, "app-state.141")); ui.add(egui::DragValue::new(&mut self.print_state.settings.margins.right_mm).range(3.0..=40.0)); });
                    ui.add_space(12.0);
                    if let Some(progress)=self.print_state.progress() { ui.add(egui::ProgressBar::new(progress).show_percentage().text(i18n::registered(self.locale, "app-state.142"))); if ui.button(i18n::registered(self.locale, "app-state.143")).clicked() { self.print_state.cancel(); } }
                    else if ui.button(i18n::registered(self.locale, "app-state.035")).clicked() && let Some(path)=rfd::FileDialog::new().set_file_name(match self.print_state.kind { drill_export::report::ReportKind::SetChart=>"set_charts.pdf",drill_export::report::ReportKind::PerformerDrillBook=>"drill_book.pdf",drill_export::report::ReportKind::CountSheet=>"count_sheet.pdf",drill_export::report::ReportKind::ProductionSheet=>"production_sheet.pdf" }).add_filter("PDF", &["pdf"]).save_file() { self.print_state.start(&self.document,self.beats_per_measure,&selected,path,self.locale,self.underlay_state.asset_bytes.clone()); }
                    if !self.print_state.status.is_empty() { ui.label(&self.print_state.status); }
                    if let Some((path,_))=&self.print_state.completed && ui.button(i18n::registered(self.locale, "app-state.144")).clicked() { let target=path.parent().unwrap_or(path); let _=std::process::Command::new("explorer").arg(target).spawn(); }

                    let ui=&mut columns[1]; ui.strong(i18n::registered(self.locale, "app-state.036"));
                    let pages=self.print_state.page_count(&self.document,self.beats_per_measure,&selected);
                    let (w,h)=self.print_state.settings.dimensions_mm(); ui.label(format!("{pages} ページ · {w:.1} × {h:.1} mm"));
                    let available=ui.available_width().min(300.0); let ratio=h/w; let (rect,_)=ui.allocate_exact_size(Vec2::new(available, (available*ratio).min(430.0)),Sense::hover());
                    ui.painter().rect_filled(rect,3.0,Color32::from_gray(245)); ui.painter().rect_stroke(rect,3.0,Stroke::new(1.0,Color32::from_gray(100)),StrokeKind::Inside);
                    let inner=rect.shrink(16.0); ui.painter().text(inner.left_top(),egui::Align2::LEFT_TOP,&self.document.title,egui::FontId::proportional(13.0),Color32::from_gray(25));
                    let report_name=match self.print_state.kind { drill_export::report::ReportKind::SetChart=>"SET CHARTS",drill_export::report::ReportKind::PerformerDrillBook=>"PERFORMER DRILL BOOK",drill_export::report::ReportKind::CountSheet=>"COUNT SHEET",drill_export::report::ReportKind::ProductionSheet=>"PRODUCTION SHEET" };
                    ui.painter().text(Pos2::new(inner.left(),inner.top()+28.0),egui::Align2::LEFT_TOP,report_name,egui::FontId::monospace(10.0),Color32::from_gray(60));
                    for row in 0..8 { let y=inner.top()+55.0+row as f32*18.0; ui.painter().line_segment([Pos2::new(inner.left(),y),Pos2::new(inner.right(),y)],Stroke::new(1.0,Color32::from_gray(190))); }
                    ui.small("ページ内容の概要表示です。PDFでは実寸レイアウトとフィールド図を生成します。");
                });
            });
        self.print_state.open = open;
        if notation_changed {
            let mut grid = self.document.grid.clone();
            grid.coordinate_notation = notation;
            self.execute_edit(
                Edit::ReplaceGrid {
                    grid,
                    scale_positions: false,
                },
                "座標表記を更新できませんでした",
            );
        }
    }
    fn new(creation: &eframe::CreationContext<'_>) -> Self {
        Self {
            onboarding: onboarding::OnboardingState::load(),
            gpu: gpu_bridge::Bridge::install(creation),
            ..Self::default()
        }
    }

    fn execute_edit(&mut self, edit: Edit, failure: &str) -> bool {
        match self.history.execute(&mut self.document, edit) {
            Ok(()) => {
                self.dirty = true;
                true
            }
            Err(_) => {
                self.status = failure.into();
                false
            }
        }
    }

    fn show_subset_snapshot_workspace(&mut self, context: &egui::Context) {
        let selected_ids = self
            .selected
            .iter()
            .filter_map(|&index| self.document.performers.get(index).map(|p| p.id))
            .collect::<BTreeSet<_>>();
        let action =
            self.subset_snapshot_state
                .show(context, self.locale, &self.document, &selected_ids);
        let Some(action) = action else { return };
        use subset_snapshot_state::Action;
        match action {
            Action::AddSubset { name } => {
                let raw = self
                    .document
                    .subsets
                    .iter()
                    .map(|s| s.id.get())
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                let Some(id) = drill_core::SubsetId::new(raw) else {
                    self.status = i18n::registered(self.locale, "app-state.037").into();
                    return;
                };
                let subset = drill_core::Subset {
                    id,
                    name,
                    members: selected_ids.iter().copied().collect(),
                };
                let failure = i18n::registered(self.locale, "app-state.038");
                if self.execute_edit(Edit::AddSubset { subset, at: None }, failure) {
                    self.subset_snapshot_state.subset_added();
                }
            }
            Action::RenameSubset { id, name } => {
                let failure = i18n::registered(self.locale, "app-state.039");
                self.execute_edit(Edit::RenameSubset { id, name }, failure);
            }
            Action::SetMembers { id, members } => {
                let failure = i18n::registered(self.locale, "app-state.040");
                self.execute_edit(Edit::SetSubsetMembers { id, members }, failure);
            }
            Action::SelectMembers { id } => {
                let member_ids = self
                    .document
                    .subsets
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.members.iter().copied().collect::<BTreeSet<_>>())
                    .unwrap_or_default();
                self.selected = self
                    .document
                    .performers
                    .iter()
                    .enumerate()
                    .filter_map(|(index, performer)| {
                        member_ids.contains(&performer.id).then_some(index)
                    })
                    .collect();
                self.status = if self.locale == Locale::Ja {
                    format!("サブセットから{}人を選択しました", self.selected.len())
                } else {
                    format!(
                        "Selected {} performers from the subset",
                        self.selected.len()
                    )
                };
            }
            Action::RemoveSubset { id } => {
                let failure = i18n::registered(self.locale, "app-state.041");
                self.execute_edit(Edit::RemoveSubset { id }, failure);
            }
            Action::CaptureSnapshot { name } => {
                match self.subset_snapshot_state.captured(name, &self.document) {
                    Ok(()) => self.status = i18n::registered(self.locale, "app-state.042").into(),
                    Err(error) => self.status = error.localized(self.locale).into(),
                }
            }
            Action::RestoreSnapshot { index } => {
                let Some(snapshot) = self.subset_snapshot_state.snapshot(index).cloned() else {
                    return;
                };
                let failure = i18n::registered(self.locale, "app-state.043");
                if self.execute_edit(
                    Edit::ReplaceDocument {
                        document: Box::new(snapshot.document),
                    },
                    failure,
                ) {
                    self.current_set = 0;
                    self.count_position = 0.0;
                    self.playback_start = 0;
                    self.playback_end = self.document.timeline_counts();
                    self.playing = false;
                    self.audio_state.pause();
                    self.selected.clear();
                    self.status = if self.locale == Locale::Ja {
                        format!("「{}」を復元しました。Undoで戻せます", snapshot.name)
                    } else {
                        format!("Restored “{}”. Undo is available", snapshot.name)
                    };
                }
            }
            Action::ForkBranch { name } => {
                match self.subset_snapshot_state.forked(name, &self.document) {
                    Ok(()) => self.status = i18n::registered(self.locale, "app-state.044").into(),
                    Err(error) => self.status = error.localized(self.locale).into(),
                }
            }
            Action::CheckpointBranch => {
                match self.subset_snapshot_state.checkpoint(&self.document) {
                    Ok(()) => self.status = i18n::registered(self.locale, "app-state.045").into(),
                    Err(error) => self.status = error.localized(self.locale).into(),
                }
            }
            Action::SwitchBranch { id } => {
                let Some(document) = self.subset_snapshot_state.branch_document(id) else {
                    return;
                };
                let failure = i18n::registered(self.locale, "app-state.046");
                if self.execute_edit(
                    Edit::ReplaceDocument {
                        document: Box::new(document),
                    },
                    failure,
                ) {
                    if let Err(error) = self.subset_snapshot_state.switched(id) {
                        self.status = error.localized(self.locale).into();
                        return;
                    }
                    self.current_set = 0;
                    self.count_position = 0.0;
                    self.playback_start = 0;
                    self.playback_end = self.document.timeline_counts();
                    self.playing = false;
                    self.audio_state.pause();
                    self.selected.clear();
                    self.status = i18n::registered(self.locale, "app-state.047").into();
                }
            }
            Action::MergeBranch { id, use_theirs } => {
                let mut preview = match self.subset_snapshot_state.merge_candidate(id) {
                    Ok(value) => value,
                    Err(error) => {
                        self.status = error.localized(self.locale).into();
                        return;
                    }
                };
                if let Err(error) = self
                    .subset_snapshot_state
                    .resolve_merge(&mut preview, use_theirs)
                {
                    self.status = error.localized(self.locale).into();
                    return;
                }
                let failure = i18n::registered(self.locale, "app-state.048");
                if self.execute_edit(
                    Edit::ReplaceDocument {
                        document: Box::new(preview.candidate.clone()),
                    },
                    failure,
                ) {
                    match self.subset_snapshot_state.merged(&preview, &self.document) {
                        Ok(()) => {
                            self.status = i18n::registered(self.locale, "app-state.049").into()
                        }
                        Err(error) => self.status = error.localized(self.locale).into(),
                    }
                }
            }
        }
    }

    fn show_section_manager(&mut self, context: &egui::Context) {
        let action =
            self.section_manager
                .show(context, self.locale, &self.document, &self.selected);
        match action {
            Some(section_manager::Action::Add { name, short }) => {
                let next = self
                    .document
                    .sections
                    .iter()
                    .map(|section| section.id.get())
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                if let Some(id) = drill_core::SectionId::new(next) {
                    let palette = [
                        [64, 180, 255],
                        [255, 112, 132],
                        [120, 220, 145],
                        [190, 135, 255],
                    ];
                    let section = drill_core::Section {
                        id,
                        name,
                        short,
                        color: palette[self.document.sections.len() % palette.len()],
                        order: u16::try_from(self.document.sections.len()).unwrap_or(u16::MAX),
                    };
                    if self.execute_edit(
                        Edit::AddSection { section, at: None },
                        i18n::registered(self.locale, "app-state.050"),
                    ) {
                        self.section_manager.section_added();
                    }
                }
            }
            Some(section_manager::Action::Rename(id, name, short)) => {
                let failure = i18n::registered(self.locale, "app-state.051");
                self.execute_edit(Edit::RenameSection { id, name, short }, failure);
            }
            Some(section_manager::Action::Assign(section)) => {
                let assignments = self
                    .selected
                    .iter()
                    .filter_map(|&index| {
                        self.document.performers.get(index).map(|p| (p.id, section))
                    })
                    .collect();
                let failure = i18n::registered(self.locale, "app-state.052");
                self.execute_edit(Edit::AssignPerformersToSection { assignments }, failure);
            }
            Some(section_manager::Action::Remove(id, reassign_to))
                if self.execute_edit(
                    Edit::RemoveSection { id, reassign_to },
                    i18n::registered(self.locale, "app-state.053"),
                ) =>
            {
                self.section_manager.section_removed(id);
            }
            Some(section_manager::Action::Remove(..)) | None => {}
        }
    }

    fn seek_global(&mut self, count: f32) {
        let (set_index, local_count) = self.document.locate_count(count);
        self.current_set = set_index;
        self.count_position = local_count;
    }

    fn toggle_playback(&mut self, context: &egui::Context) {
        if self.playing {
            self.playing = false;
            self.audio_state.pause();
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
            self.ever_played = true;
            if let Some(track) = &self.document.audio {
                let global = self
                    .document
                    .global_count(self.current_set, self.count_position);
                let seconds =
                    drill_core::audio::count_to_audio_time(track, &self.document.tempo, global);
                self.audio_state.seek_seconds(seconds);
                self.audio_state.set_mix(track.gain_linear(), track.muted);
                self.audio_state.play();
            }
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
        let before = self.selected_points();
        let after = points
            .into_iter()
            .map(|point| self.document.grid.snap(point))
            .collect::<Vec<_>>();
        if before == after {
            return;
        }
        let performer_ids = self
            .selected
            .iter()
            .filter_map(|&index| self.document.performers.get(index).map(|p| p.id))
            .collect();
        let set_id = self.document.sets[self.current_set].id;
        self.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids,
                positions: after,
            },
            i18n::registered(self.locale, "app-state.054"),
        );
    }

    /// Applies a Formation Designer result as one validated undo transaction,
    /// retaining its parametric source for later editing.
    fn commit_shape(&mut self, spec: shapes::ShapeSpec) {
        if self.selected.is_empty() || spec.validate().is_err() {
            self.status = i18n::registered(self.locale, "app-state.055").into();
            return;
        }
        let mut sampled = Vec::with_capacity(self.selected.len());
        spec.sample(self.selected.len(), &mut sampled);
        let current = self.selected_points();
        let assignment = pathing::optimal_assignment(&current, &sampled);
        let mut next = self.document.clone();
        let grid = next.grid.clone();
        let Some(set) = next.sets.get_mut(self.current_set) else {
            return;
        };
        for (rank, &index) in self.selected.iter().enumerate() {
            if let Some(&target) = assignment.get(rank).and_then(|&i| sampled.get(i)) {
                set.positions[index] = grid.snap(target);
            }
        }
        set.shape = Some(spec);
        if self.execute_edit(
            Edit::ReplaceDocument {
                document: Box::new(next),
            },
            i18n::registered(self.locale, "app-state.056"),
        ) {
            self.status = i18n::registered(self.locale, "app-state.057").into();
        }
    }

    fn preview_shape(&mut self, spec: shapes::ShapeSpec) {
        if self.selected.is_empty() || spec.validate().is_err() {
            return;
        }
        spec.sample(self.selected.len(), &mut self.formation_preview_points);
        self.formation_preview_spec = Some(spec);
    }

    fn apply_shape_preview(&mut self) {
        if let Some(spec) = self.formation_preview_spec.take() {
            self.formation_preview_points.clear();
            self.free_draw_active = false;
            self.free_draw_raw.clear();
            self.commit_shape(spec);
        }
    }

    fn cancel_shape_preview(&mut self) {
        self.formation_preview_spec = None;
        self.formation_preview_points.clear();
        self.free_draw_active = false;
        self.free_draw_raw.clear();
        self.status = i18n::registered(self.locale, "app-state.058").into();
    }

    fn begin_free_draw(&mut self) {
        self.formation_preview_spec = None;
        self.formation_preview_points.clear();
        self.free_draw_raw.clear();
        self.free_draw_active = true;
        self.status = i18n::registered(self.locale, "app-state.059").into();
    }

    fn preview_formation_text(&mut self) {
        const NOTO_SANS_JP: &[u8] = include_bytes!("../../../assets/NotoSansJP.ttf");
        let points = self.selected_points();
        if points.is_empty() {
            return;
        }
        let min_x = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let min_y = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let max_y = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
        let options = drill_interop::font_outline::TextOutlineOptions {
            origin: Point { x: min_x, y: min_y },
            height: (max_y - min_y).max(8.0),
            ..Default::default()
        };
        match drill_interop::font_outline::outline_text(NOTO_SANS_JP, &self.formation_text, options)
        {
            Ok(outline) => {
                let warnings = outline.warnings.len();
                self.preview_shape(outline.shape);
                self.status = if warnings == 0 {
                    i18n::registered(self.locale, "app-state.060").into()
                } else {
                    format!(
                        "{}: {warnings}",
                        i18n::registered(self.locale, "app-state.061")
                    )
                };
            }
            Err(error) => {
                self.formation_preview_spec = None;
                self.formation_preview_points.clear();
                self.status = format!(
                    "{}: {error}",
                    i18n::registered(self.locale, "app-state.062")
                );
            }
        }
    }

    fn finish_free_draw_preview(&mut self) {
        self.free_draw_active = false;
        let tolerance = (self.document.grid.width / 800.0).max(0.05);
        let vertices = shapes::simplify_free_path(&self.free_draw_raw, tolerance);
        self.free_draw_raw.clear();
        if vertices.len() < 2 {
            self.status = i18n::registered(self.locale, "app-state.063").into();
            return;
        }
        self.preview_shape(shapes::ShapeSpec::FreePath { vertices });
        self.status = i18n::registered(self.locale, "app-state.064").into();
    }

    fn commit_designer_positions(
        &mut self,
        positions: Vec<Point>,
        shape: Option<shapes::ShapeSpec>,
        success_ja: &'static str,
        success_en: &'static str,
    ) {
        if positions.len() != self.selected.len()
            || positions
                .iter()
                .any(|p| !p.x.is_finite() || !p.y.is_finite())
        {
            self.status = i18n::registered(self.locale, "app-state.065").into();
            return;
        }
        let mut next = self.document.clone();
        let grid = next.grid.clone();
        let Some(set) = next.sets.get_mut(self.current_set) else {
            return;
        };
        for (&index, point) in self.selected.iter().zip(positions) {
            set.positions[index] = grid.snap(point);
        }
        set.shape = shape;
        if self.execute_edit(
            Edit::ReplaceDocument {
                document: Box::new(next),
            },
            i18n::registered(self.locale, "app-state.066"),
        ) {
            self.status = tr(self.locale, success_ja, success_en).into();
        }
    }

    fn apply_morph_preview(&mut self, amount: f32) {
        let Some(next_set) = self.document.sets.get(self.current_set + 1) else {
            self.status = i18n::registered(self.locale, "app-state.067").into();
            return;
        };
        let from = self.selected_points();
        let to = self
            .selected
            .iter()
            .filter_map(|&i| next_set.positions.get(i).copied())
            .collect::<Vec<_>>();
        let preview = shapes::morph(&from, &to, amount);
        self.commit_designer_positions(
            preview,
            None,
            "次セットへのモーフを適用しました",
            "Morph toward next set applied",
        );
    }

    fn apply_radial_selection(&mut self, fold: u32) {
        use std::collections::BTreeMap;
        let ids = self
            .selected
            .iter()
            .filter_map(|&i| self.document.performers.get(i).map(|p| p.id))
            .collect::<Vec<_>>();
        if ids.len() < fold as usize {
            self.status = i18n::registered(self.locale, "app-state.068").into();
            return;
        }
        let center = editing::centroid(&self.selected_points());
        let masters = ids.len().div_ceil(fold as usize);
        let groups = (0..masters)
            .map(|i| {
                (0..fold as usize)
                    .filter_map(|k| ids.get(i + k * masters).copied())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut map: BTreeMap<_, _> = ids
            .iter()
            .zip(self.selected_points())
            .map(|(&id, p)| (id, p))
            .collect();
        shapes::apply_radial(center, fold, &groups, &mut map);
        let points = ids.iter().filter_map(|id| map.get(id).copied()).collect();
        self.commit_designer_positions(
            points,
            None,
            "放射対称を適用しました",
            "Radial symmetry applied",
        );
    }

    fn apply_constraint_cleanup(&mut self) {
        use drill_core::constraint_solver::{Constraint, SolverLimits, solve_constraints};
        let initial = self.selected_points();
        if initial.len() < 2 {
            self.status = i18n::registered(self.locale, "app-state.069").into();
            return;
        }
        let mut constraints = vec![
            Constraint::InsideField {
                min: Point { x: 0.0, y: 0.0 },
                max: Point {
                    x: self.document.grid.width,
                    y: self.document.grid.height,
                },
            },
            Constraint::MinimumDistance { distance: 1.5 },
        ];
        if self.current_set > 0 {
            let previous = &self.document.sets[self.current_set - 1].positions;
            let origins = self
                .selected
                .iter()
                .filter_map(|&i| previous.get(i).copied())
                .collect::<Vec<_>>();
            if origins.len() == initial.len() {
                constraints.push(Constraint::MaximumStep {
                    origins,
                    distance: f32::from(self.document.sets[self.current_set].counts).max(1.0) * 1.5,
                });
            }
        }
        if let Some(spec) = self.document.sets[self.current_set].shape.as_ref() {
            let mut targets = Vec::new();
            spec.sample(initial.len(), &mut targets);
            if targets.len() == initial.len() {
                constraints.push(Constraint::ShapeFollow {
                    targets,
                    strength: 0.15,
                });
            }
        }
        match solve_constraints(&initial, &constraints, SolverLimits::default(), |_| true) {
            Ok(proposal) => {
                let converged = proposal.converged;
                let iterations = proposal.iterations;
                self.commit_layout(proposal.positions);
                self.status = format!(
                    "{} · {} {}{}",
                    i18n::registered(self.locale, "app-state.070"),
                    iterations,
                    i18n::registered(self.locale, "app-state.071"),
                    if converged {
                        ""
                    } else {
                        i18n::registered(self.locale, "app-state.072")
                    }
                );
            }
            Err(error) => {
                self.status = format!(
                    "{}: {error}",
                    i18n::registered(self.locale, "app-state.073")
                )
            }
        }
    }

    fn apply_section_shape_assignment(&mut self) {
        use std::collections::BTreeMap;
        let Some(spec) = self
            .document
            .sets
            .get(self.current_set)
            .and_then(|s| s.shape.clone())
        else {
            self.status = i18n::registered(self.locale, "app-state.074").into();
            return;
        };
        let selected_ids = self
            .selected
            .iter()
            .filter_map(|&i| self.document.performers.get(i).map(|p| p.id))
            .collect::<Vec<_>>();
        let mut groups = Vec::new();
        for section in &self.document.sections {
            let performers = selected_ids
                .iter()
                .copied()
                .filter(|id| {
                    self.document
                        .performers
                        .iter()
                        .any(|p| p.id == *id && p.section == section.id)
                })
                .collect::<Vec<_>>();
            if !performers.is_empty() {
                groups.push(shapes::AssignmentGroup {
                    section: section.id,
                    performers,
                });
            }
        }
        let current: BTreeMap<_, _> = selected_ids
            .iter()
            .zip(self.selected_points())
            .map(|(&id, p)| (id, p))
            .collect();
        let mut targets = Vec::new();
        spec.sample(selected_ids.len(), &mut targets);
        let (assigned, unplaced) = shapes::assign_to_shape(&groups, &targets, &current);
        if !unplaced.is_empty() || assigned.len() != selected_ids.len() {
            self.status = i18n::registered(self.locale, "app-state.075").into();
            return;
        }
        let points = selected_ids
            .iter()
            .filter_map(|id| assigned.get(id).copied())
            .collect();
        self.commit_designer_positions(
            points,
            Some(spec),
            "セクションを保って再配置しました",
            "Reassigned while preserving sections",
        );
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

    fn export_text(&mut self, default_name: &str, filter_name: &str, ext: &str, contents: String) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(filter_name, &[ext])
            .set_file_name(default_name)
            .save_file()
        {
            match self.text_export_state.start(path.clone(), contents) {
                Ok(()) => {
                    self.status = format!(
                        "{}: {}",
                        i18n::registered(self.locale, "app-state.076"),
                        path.display()
                    )
                }
                Err(error) => {
                    self.status = format!(
                        "{}: {}",
                        i18n::registered(self.locale, "app-state.077"),
                        error.localized(self.locale)
                    )
                }
            }
        }
    }

    /// Reassign the next set's dots to the current performers so total travel is
    /// minimized, keeping the target formation shape but reducing crossings.
    fn auto_assign_next(&mut self) {
        let next_index = self.current_set + 1;
        let Some(next) = self.document.sets.get(next_index) else {
            self.status = i18n::registered(self.locale, "app-state.145").into();
            return;
        };
        let from = &self.document.sets[self.current_set].positions;
        let before = next.positions.clone();
        let assignment = pathing::optimal_assignment(from, &before);
        let after = assignment.iter().map(|&j| before[j]).collect::<Vec<_>>();
        if after == before {
            self.status = i18n::registered(self.locale, "app-state.146").into();
            return;
        }
        let set_id = self.document.sets[next_index].id;
        let performer_ids = self.document.performers.iter().map(|p| p.id).collect();
        if self.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids,
                positions: after,
            },
            i18n::registered(self.locale, "app-state.078"),
        ) {
            self.status = i18n::registered(self.locale, "app-state.079").into();
        }
    }

    /// Read-only 3D stadium visualization of the current frame. Editing stays in 2D.
    fn save_dialog(&mut self) {
        let path = self
            .current_path
            .clone()
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .or_else(|| {
                rfd::FileDialog::new()
                    .add_filter("DrillForge", &["drill.json"])
                    .set_file_name("untitled.drill.json")
                    .save_file()
            });
        if let Some(path) = path {
            self.project_state
                .save_legacy_json(path, self.document.clone());
        }
    }

    fn open_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DrillForge", &["json"])
            .pick_file()
        else {
            return;
        };
        self.project_state.load_legacy_json(path);
    }
}

impl DrillApp {
    fn import_coordinates_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Coordinate table", &["csv", "tsv", "txt", "xlsx"])
            .pick_file()
        {
            self.import_state.choose(path);
        }
    }

    fn show_musical_import_window(&mut self, context: &egui::Context) {
        if !self.import_state.musical_open {
            return;
        }
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(i18n::registered(self.locale, "app-state.080"))
            .collapsible(false)
            .show(context, |ui| {
                ui.label(self.import_state.status.text(self.locale));
                if let Some((source, result)) = self.import_state.musical_review() {
                    ui.heading(source);
                    ui.label(format!(
                        "{}: {}",
                        i18n::registered(self.locale, "app-state.081"),
                        result.timeline.tempo.events().len()
                    ));
                    ui.label(format!(
                        "{}: {} / {}: {}",
                        i18n::registered(self.locale, "app-state.082"),
                        result.timeline.meters.len(),
                        i18n::registered(self.locale, "app-state.083"),
                        result.timeline.total_counts
                    ));
                    ui.label(format!(
                        "{}: {} / {}: {}",
                        i18n::registered(self.locale, "app-state.084"),
                        result.timeline.measures.len(),
                        i18n::registered(self.locale, "app-state.085"),
                        result.timeline.marks.len()
                    ));
                    ui.small(i18n::registered(self.locale, "app-state.086"));
                    for event in result.timeline.tempo.events().iter().take(12) {
                        ui.monospace(format!(
                            "Count {:>7.2}  {:>7.2} BPM",
                            event.count, event.bpm
                        ));
                    }
                    if result.timeline.tempo.events().len() > 12 {
                        ui.small("…");
                    }
                    if !result.warnings.is_empty() {
                        ui.colored_label(
                            Color32::YELLOW,
                            format!(
                                "{}: {:?}",
                                i18n::registered(self.locale, "app-state.087"),
                                result.warnings
                            ),
                        );
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .button(i18n::registered(self.locale, "app-state.088"))
                            .clicked()
                        {
                            apply = true;
                        }
                        if ui
                            .button(i18n::registered(self.locale, "app-state.089"))
                            .clicked()
                        {
                            cancel = true;
                        }
                    });
                } else if ui
                    .button(i18n::registered(self.locale, "app-state.090"))
                    .clicked()
                {
                    cancel = true;
                }
            });
        if apply && let Some(timeline) = self.import_state.take_musical_timeline() {
            let mut document = self.document.clone();
            document.tempo = timeline.tempo;
            let starts = (0..document.sets.len())
                .map(|i| document.global_count(i, 0.0))
                .collect::<Vec<_>>();
            for (i, set) in document.sets.iter_mut().enumerate() {
                set.annotation.tempo_bpm = Some(document.tempo.bpm_at(starts[i]));
            }
            for mark in timeline.marks {
                if let Some(index) = starts
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        (mark.count - **a)
                            .abs()
                            .total_cmp(&(mark.count - **b).abs())
                    })
                    .map(|(i, _)| i)
                {
                    document.sets[index].annotation.rehearsal_mark = mark.text;
                }
            }
            match self.history.execute(
                &mut self.document,
                Edit::ReplaceDocument {
                    document: Box::new(document),
                },
            ) {
                Ok(()) => {
                    self.tempo_bpm = self.document.tempo.bpm_at(0.0);
                    self.tempo_draft = Some(self.document.tempo.clone());
                    self.tempo_draft_dirty = false;
                    self.dirty = true;
                    self.status = i18n::registered(self.locale, "app-state.091").into();
                }
                Err(error) => {
                    self.status = format!(
                        "{}: {error}",
                        i18n::registered(self.locale, "app-state.092")
                    )
                }
            }
        }
        if cancel {
            self.import_state.cancel_musical();
        }
    }

    fn show_import_window(&mut self, context: &egui::Context) {
        if !self.import_state.open {
            return;
        }
        let mut open = true;
        let preview = self.import_state.preview().cloned();
        let xlsx_names = self.import_state.xlsx_sheet_names();
        let selected_xlsx = self.import_state.selected_xlsx_sheet();
        let phrase_diagnostics = self.import_state.phrase_preview(&self.document.grid);
        let mut choose_xlsx = None;
        let locale = self.locale;
        let mut start_review = false;
        let mut request_confirmation = false;
        let mut apply = false;
        egui::Window::new(i18n::registered(self.locale, "app-state.093"))
            .open(&mut open)
            .resizable(true)
            .default_width(760.0)
            .show(context, |ui| {
                ui.heading(
                    self.import_state
                        .source_name()
                        .unwrap_or_else(|| "座標表".into()),
                );
                ui.label(i18n::registered(self.locale, "app-state.094"));
                ui.label(self.import_state.status.text(self.locale));
                if let Some(preview) = &preview {
                    ui.separator();
                    if let Some(selected) = selected_xlsx {
                        ui.horizontal(|ui| {
                            ui.label(i18n::registered(locale, "app-state.095"));
                            egui::ComboBox::from_id_salt("xlsx-sheet")
                                .selected_text(xlsx_names.get(selected).map_or("?", String::as_str))
                                .show_ui(ui, |ui| {
                                    for (i, name) in xlsx_names.iter().enumerate() {
                                        if ui.selectable_label(i == selected, name).clicked() {
                                            choose_xlsx = Some(i);
                                        }
                                    }
                                });
                        });
                    }
                    ui.label(format!(
                        "{}: {} · {}: {}{}",
                        i18n::registered(self.locale, "app-state.097"),
                        preview.total_rows,
                        i18n::registered(self.locale, "app-state.098"),
                        match preview.delimiter {
                            drill_interop::Delimiter::Comma => "CSV (,)",
                            drill_interop::Delimiter::Tab => "TSV (Tab)",
                            drill_interop::Delimiter::Semicolon => "CSV (;)",
                        },
                        if preview.truncated {
                            i18n::registered(self.locale, "app-state.096")
                        } else {
                            ""
                        }
                    ));
                    if preview.replacement_characters {
                        ui.colored_label(
                            Color32::YELLOW,
                            i18n::registered(self.locale, "app-state.099"),
                        );
                    }
                    if let Some(mapping) = self.import_state.mapping_mut() {
                        egui::Grid::new("import-column-map")
                            .num_columns(2)
                            .striped(true)
                            .show(ui, |ui| {
                                for (label, index) in [
                                    ("Performer / 演者", &mut mapping.performer),
                                    ("Set / セット", &mut mapping.set),
                                    ("X / 横・左右座標文", &mut mapping.x),
                                    ("Y / 縦・前後座標文", &mut mapping.y),
                                ] {
                                    ui.label(label);
                                    egui::ComboBox::from_id_salt(label)
                                        .selected_text(
                                            preview.headers.get(*index).map_or("?", String::as_str),
                                        )
                                        .show_ui(ui, |ui| {
                                            for (i, header) in preview.headers.iter().enumerate() {
                                                ui.selectable_value(index, i, header);
                                            }
                                        });
                                    ui.end_row();
                                }
                                ui.label(i18n::registered(self.locale, "app-state.147"));
                                optional_column_combo(
                                    ui,
                                    "import-counts",
                                    &preview.headers,
                                    &mut mapping.counts,
                                );
                                ui.end_row();
                                ui.label(i18n::registered(self.locale, "app-state.148"));
                                optional_column_combo(
                                    ui,
                                    "import-section",
                                    &preview.headers,
                                    &mut mapping.section,
                                );
                                ui.end_row();
                            });
                    }
                    if !phrase_diagnostics.is_empty() {
                        ui.label(i18n::registered(locale, "app-state.100"));
                        for diagnostic in phrase_diagnostics.iter().take(8) {
                            if let Some(point) = diagnostic.point {
                                ui.colored_label(
                                    Color32::LIGHT_GREEN,
                                    format!(
                                        "✓ {} {} → ({:.2}, {:.2})",
                                        i18n::registered(locale, "app-state.101"),
                                        diagnostic.line,
                                        point.x,
                                        point.y
                                    ),
                                );
                            } else if let Some(error) = &diagnostic.error {
                                ui.colored_label(
                                    Color32::YELLOW,
                                    format!(
                                        "⚠ {} {}: {error}",
                                        i18n::registered(locale, "app-state.102"),
                                        diagnostic.line
                                    ),
                                );
                            }
                        }
                    }
                    ui.separator();
                    ui.label(i18n::registered(self.locale, "app-state.103"));
                    egui::ScrollArea::both().max_height(230.0).show(ui, |ui| {
                        egui::Grid::new("import-preview-grid")
                            .striped(true)
                            .show(ui, |ui| {
                                for header in &preview.headers {
                                    ui.strong(header);
                                }
                                ui.end_row();
                                for row in &preview.rows {
                                    for i in 0..preview.headers.len() {
                                        ui.label(row.get(i).map_or("", String::as_str));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
                    ui.label(i18n::registered(self.locale, "app-state.104"));
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.import_state.busy(),
                                egui::Button::new(i18n::registered(self.locale, "app-state.105")),
                            )
                            .clicked()
                        {
                            start_review = true;
                        }
                        if ui
                            .button(i18n::registered(self.locale, "app-state.106"))
                            .clicked()
                        {
                            self.import_state.cancel();
                        }
                    });
                } else if let Some(review) = self.import_state.review_mut() {
                    let diff = &review.diff;
                    ui.separator();
                    ui.heading(i18n::registered(locale, "app-state.107"));
                    ui.label(format!(
                        "{} +{}  ·  {} +{} / Δ{}  ·  {} {}",
                        i18n::registered(locale, "app-state.108"),
                        diff.performers_added,
                        i18n::registered(locale, "app-state.109"),
                        diff.sets_added,
                        diff.sets_changed,
                        diff.coordinates_moved,
                        i18n::registered(locale, "app-state.110")
                    ));
                    if diff.performers_changed > 0 {
                        ui.label(format!(
                            "{} Δ{}",
                            i18n::registered(locale, "app-state.111"),
                            diff.performers_changed
                        ));
                    }
                    for warning in &diff.warnings {
                        ui.colored_label(Color32::YELLOW, format!("⚠ {warning}"));
                    }
                    for warning in
                        review
                            .outcome
                            .report
                            .warnings
                            .iter()
                            .map(|warning| match warning {
                                drill_interop::ImportWarning::CountsDefaulted { .. } => {
                                    "Counts defaulted for one or more sets"
                                }
                                drill_interop::ImportWarning::HeldPreviousSet { .. } => {
                                    "Missing rows use a neighboring set position"
                                }
                                drill_interop::ImportWarning::SectionCreated(_) => {
                                    "Imported sections apply with selected performer rows"
                                }
                                drill_interop::ImportWarning::ReplacementCharacters => {
                                    "Replacement characters were detected"
                                }
                            })
                    {
                        ui.colored_label(Color32::YELLOW, format!("⚠ {warning}"));
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .small_button(i18n::registered(locale, "app-state.112"))
                            .clicked()
                        {
                            review.selection = drill_interop::DiffSelection::all(diff);
                        }
                        if ui
                            .small_button(i18n::registered(locale, "app-state.113"))
                            .clicked()
                        {
                            review.selection = drill_interop::DiffSelection::none(diff);
                        }
                    });
                    ui.label(i18n::registered(locale, "app-state.114"));
                    egui::ScrollArea::horizontal()
                        .max_height(70.0)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for (index, set) in review.outcome.document.sets.iter().enumerate()
                                {
                                    ui.checkbox(&mut review.selection.sets[index], &set.name);
                                }
                            });
                        });
                    ui.label(i18n::registered(locale, "app-state.115"));
                    let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
                    egui::ScrollArea::vertical().max_height(260.0).show_rows(
                        ui,
                        row_height,
                        diff.rows.len(),
                        |ui, range| {
                            for index in range {
                                let row = diff.rows[index];
                                let set = &review.outcome.document.sets[row.imported_set].name;
                                let performer = &review.outcome.document.performers
                                    [row.imported_performer]
                                    .label;
                                ui.horizontal(|ui| {
                                    ui.add_enabled_ui(
                                        !review.selection.sets[row.imported_set],
                                        |ui| {
                                            ui.checkbox(&mut review.selection.rows[index], "");
                                        },
                                    );
                                    ui.monospace(format!("{set} · {performer}"));
                                    match row.before {
                                        Some(before) => ui.label(format!(
                                            "({:.2}, {:.2}) → ({:.2}, {:.2})",
                                            before.x, before.y, row.after.x, row.after.y
                                        )),
                                        None => ui.label(format!(
                                            "{} ({:.2}, {:.2})",
                                            i18n::registered(locale, "app-state.116"),
                                            row.after.x,
                                            row.after.y
                                        )),
                                    };
                                });
                            }
                        },
                    );
                    let selected_rows = diff
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(i, row)| {
                            review.selection.sets[row.imported_set] || review.selection.rows[*i]
                        })
                        .count();
                    let selected_set_changes =
                        review.selection.sets.iter().any(|selected| *selected)
                            && (diff.sets_added > 0
                                || diff.sets_changed > 0
                                || diff.performers_changed > 0);
                    ui.label(format!(
                        "{}: {}",
                        i18n::registered(locale, "app-state.117"),
                        selected_rows
                    ));
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                (selected_rows > 0 || selected_set_changes)
                                    && !self.import_state.busy(),
                                egui::Button::new(i18n::registered(locale, "app-state.118")),
                            )
                            .clicked()
                        {
                            request_confirmation = true;
                        }
                        if ui
                            .button(i18n::registered(locale, "app-state.119"))
                            .clicked()
                        {
                            self.import_state.cancel();
                        }
                    });
                }
                if self.import_state.confirming {
                    ui.separator();
                    ui.colored_label(
                        Color32::from_rgb(255, 210, 90),
                        i18n::registered(locale, "app-state.120"),
                    );
                    ui.horizontal(|ui| {
                        if ui
                            .button(i18n::registered(locale, "app-state.121"))
                            .clicked()
                        {
                            apply = true;
                        }
                        if ui
                            .button(i18n::registered(locale, "app-state.122"))
                            .clicked()
                        {
                            self.import_state.confirming = false;
                        }
                    });
                }
            });
        if let Some(index) = choose_xlsx
            && let Err(error) = self.import_state.select_xlsx_sheet(index)
        {
            self.status = format!(
                "{}: {}",
                i18n::registered(locale, "app-state.123"),
                error.localized(locale)
            );
        }
        if start_review {
            self.import_state
                .start_review(self.document.grid.clone(), self.document.clone());
        }
        if request_confirmation {
            self.import_state.confirming = true;
        }
        if apply && let Err(error) = self.import_state.apply_selected(&self.document) {
            self.status = error.into();
        }
        if !open {
            self.import_state.cancel();
        }
    }
}

fn optional_column_combo(
    ui: &mut egui::Ui,
    id: &str,
    headers: &[String],
    value: &mut Option<usize>,
) {
    let selected = value
        .and_then(|i| headers.get(i))
        .map_or("—", String::as_str);
    egui::ComboBox::from_id_salt(id)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "—");
            for (i, header) in headers.iter().enumerate() {
                ui.selectable_value(value, Some(i), header);
            }
        });
}

// Kept during the staged renderer migration as a visual parity reference.
#[allow(dead_code)]
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

fn utc_month_day() -> (u8, u8) {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    (month as u8, day as u8)
}

#[cfg(test)]
mod locale_regression_tests {
    use super::*;

    fn contains_japanese(value: &str) -> bool {
        value.chars().any(|ch| {
            matches!(ch,
                '\u{3040}'..='\u{30ff}' |
                '\u{3400}'..='\u{4dbf}' |
                '\u{4e00}'..='\u{9fff}')
        })
    }

    #[test]
    fn english_primary_workflow_copy_has_no_japanese_glyphs() {
        let labels = [
            i18n::registered(Locale::En, "app-state.124"),
            i18n::registered(Locale::En, "app-state.125"),
            i18n::registered(Locale::En, "app-state.126"),
            i18n::registered(Locale::En, "app-state.127"),
            i18n::registered(Locale::En, "app-state.128"),
            i18n::registered(Locale::En, "app-state.129"),
            i18n::registered(Locale::En, "app-state.130"),
            i18n::registered(Locale::En, "app-state.131"),
            i18n::registered(Locale::En, "app-state.132"),
            i18n::registered(Locale::En, "app-state.133"),
            i18n::registered(Locale::En, "app-state.134"),
            i18n::registered(Locale::En, "app-state.135"),
        ];
        for label in labels {
            assert!(
                !contains_japanese(label),
                "Japanese leaked into English UI: {label}"
            );
        }
    }
}
