//! Deterministic, device-free acceptance smoke tests for the desktop workflow.
//!
//! These tests intentionally exercise the same controller commands and edit
//! history as the visible UI. Native window screenshots remain a separate
//! visual check; this layer catches broken wiring on every CI operating system.

#[cfg(test)]
mod tests {
    use super::super::commands::{self, Command, Menu};
    use crate::{DrillApp, Locale, Point, WorkspaceFocus};
    use drill_core::Edit;
    use eframe::egui;

    fn contains_japanese(value: &str) -> bool {
        value.chars().any(|ch| {
            matches!(ch,
                '\u{3040}'..='\u{30ff}' |
                '\u{3400}'..='\u{4dbf}' |
                '\u{4e00}'..='\u{9fff}')
        })
    }

    #[test]
    fn onboarding_to_edit_and_play_uses_real_controller_path() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();

        assert!(app.onboarding.show_welcome);
        app.onboarding.show_welcome = false;
        app.onboarding.welcome_seen = true;

        let initial_sets = app.document.sets.len();
        app.execute_command(Command::DuplicateSet, &context);
        assert_eq!(app.document.sets.len(), initial_sets + 1);

        app.execute_command(Command::SelectAll, &context);
        assert_eq!(app.selected.len(), app.document.performers.len());
        app.selected = [0].into_iter().collect();
        app.onboarding.observe(true, false, false);

        let set_id = app.document.sets[app.current_set].id;
        let performer_id = app.document.performers[0].id;
        let before = app.document.sets[app.current_set].positions[0];
        let after = Point {
            x: (before.x + 1.0).min(app.document.grid.width),
            y: before.y,
        };
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![performer_id],
                positions: vec![after],
            },
            "smoke edit failed",
        ));
        app.onboarding.observe(true, true, false);
        assert_eq!(app.document.sets[app.current_set].positions[0], after);
        assert!(app.history.can_undo());

        app.execute_command(Command::RangeWholeShow, &context);
        app.execute_command(Command::PlayPause, &context);
        app.onboarding.observe(true, true, app.ever_played);
        assert!(app.playing);
        assert!(app.ever_played);
        assert!(app.onboarding.coach_message(Locale::En).is_none());

        app.execute_command(Command::PlayPause, &context);
        assert!(!app.playing);
    }

    #[test]
    fn new_show_supports_the_core_edit_loop() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        app.begin_new_show();

        assert_eq!(app.document.performers.len(), 16);
        assert_eq!(app.document.sets.len(), 1);
        assert!(!app.dirty);
        assert!(app.current_path.is_none());
        assert!(app.is_editable_set_start());
        assert!(
            Command::DeleteSet
                .enabled(app.command_context(), app.locale)
                .is_err()
        );

        let playback_before = app.playback_end;
        app.document.sets[0].generated_by = drill_core::GeneratorId::new(99);
        app.execute_command(Command::DuplicateSet, &context);
        assert_eq!(app.document.sets.len(), 2);
        assert_eq!(app.current_set, 1);
        assert!(app.document.sets[1].generated_by.is_none());
        assert_eq!(app.playback_end, app.document.timeline_counts());
        assert!(app.playback_end > playback_before);
        assert!(app.dirty);
        assert_eq!(app.count_position, 0.0);

        let roster = app.document.performers.len();
        app.execute_command(Command::AddPerformer, &context);
        assert_eq!(app.document.performers.len(), roster + 1);
        assert!(
            app.document
                .sets
                .iter()
                .all(|set| set.positions.len() == roster + 1)
        );
        assert_eq!(app.selected.len(), 1);

        app.execute_command(Command::RemoveSelectedPerformers, &context);
        assert_eq!(app.document.performers.len(), roster);
        assert!(app.selected.is_empty());

        app.execute_command(Command::SelectAll, &context);
        app.execute_command(Command::RemoveSelectedPerformers, &context);
        assert_eq!(app.document.performers.len(), roster);
        assert_eq!(
            app.status,
            "最後の演者は削除できません。先に選択を減らしてください"
        );

        app.execute_command(Command::DeleteSet, &context);
        assert_eq!(app.document.sets.len(), 1);
        assert_eq!(app.current_set, 0);
        assert!(
            Command::DeleteSet
                .enabled(app.command_context(), app.locale)
                .is_err()
        );
        app.execute_command(Command::DeleteSet, &context);
        assert_eq!(app.document.sets.len(), 1);
    }

    #[test]
    fn duplicating_a_set_then_undoing_keeps_current_set_in_range() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.execute_command(Command::DuplicateSet, &context);
        assert_eq!(app.current_set, 1);
        app.execute_command(Command::Undo, &context);
        assert_eq!(app.document.sets.len(), 1);
        assert!(app.current_set < app.document.sets.len());
        assert_eq!(app.playback_end, app.document.timeline_counts());
        let _ = app.document.sets[app.current_set].name.as_str();
    }

    #[test]
    fn placing_a_performer_uses_the_field_pointer_and_clamps_to_the_grid() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        app.begin_new_show();
        let target = Point { x: 8.0, y: 6.0 };
        app.field_pointer = Some(target);
        let roster = app.document.performers.len();
        app.execute_command(Command::AddPerformer, &context);
        assert_eq!(app.document.performers.len(), roster + 1);
        let placed = *app.document.sets[0]
            .positions
            .last()
            .expect("new performer has a position");
        assert_eq!(
            placed,
            super::super::controller::field_point(target, &app.document, true)
        );
        assert_eq!(placed, app.document.grid.snap(placed));

        app.place_performer_at(
            Point {
                x: 10_000.0,
                y: -4.0,
            },
            true,
        );
        let clamped = *app.document.sets[0]
            .positions
            .last()
            .expect("clamped performer has a position");
        assert_eq!(clamped.x, app.document.grid.max_x());
        assert_eq!(clamped.y, 0.0);
        assert_eq!(clamped, app.document.grid.snap(clamped));
    }

    #[test]
    fn placing_on_an_occupied_spot_warns_but_still_adds() {
        let mut app = DrillApp::default();
        app.begin_new_show();
        let existing = app.document.sets[0].positions[0];
        let roster = app.document.performers.len();
        app.place_performer_at(existing, true);
        assert_eq!(app.document.performers.len(), roster + 1);
        assert_eq!(
            app.status,
            super::super::i18n::registered(Locale::Ja, "core-edit.025")
        );
    }

    #[test]
    fn escape_returns_from_place_to_select_without_a_selection() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.selected.clear();
        app.set_field_tool(super::super::FieldTool::Place);
        assert_eq!(app.field_tool, super::super::FieldTool::Place);
        assert!(
            Command::ClearSelection
                .enabled(app.command_context(), app.locale)
                .is_ok()
        );
        app.execute_command(Command::ClearSelection, &context);
        assert_eq!(app.field_tool, super::super::FieldTool::Select);
        assert_eq!(
            app.status,
            super::super::i18n::registered(Locale::Ja, "core-edit.029")
        );
    }

    #[test]
    fn playback_blocks_placing_a_performer() {
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.playing = true;
        let roster = app.document.performers.len();
        app.place_performer_at(Point { x: 5.0, y: 5.0 }, true);
        assert_eq!(app.document.performers.len(), roster);
    }

    fn wait_for_project_event(app: &mut DrillApp) -> super::super::project_state::ProjectEvent {
        for _ in 0..400 {
            if let Some(event) = app.project_state.poll() {
                return event;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("project job timed out");
    }

    #[test]
    fn create_place_save_and_reopen_roundtrips_the_document() {
        let mut app = DrillApp::default();
        app.begin_new_show();
        let target = Point { x: 8.0, y: 6.0 };
        app.place_performer_at(target, true);
        let roster = app.document.performers.len();
        let placed = *app.document.sets[0]
            .positions
            .last()
            .expect("placed performer has a position");
        let labels: Vec<String> = app
            .document
            .performers
            .iter()
            .map(|performer| performer.label.clone())
            .collect();

        let path = std::env::temp_dir().join(format!(
            "drillforge-roundtrip-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        app.project_state
            .save_legacy_json(path.clone(), app.document.clone());
        match wait_for_project_event(&mut app) {
            super::super::project_state::ProjectEvent::Saved(saved) => {
                app.apply_saved_path(saved);
            }
            super::super::project_state::ProjectEvent::Loaded { .. } => {
                panic!("expected Saved, got Loaded")
            }
            super::super::project_state::ProjectEvent::Failed(error) => {
                panic!("save failed: {error}")
            }
        }
        assert_eq!(app.current_path.as_deref(), Some(path.as_path()));
        assert!(!app.dirty);

        app.begin_new_show();
        assert_eq!(app.document.performers.len(), 16);
        assert!(app.current_path.is_none());

        app.playing = true;
        app.field_tool = super::super::FieldTool::Place;
        app.project_state.load_legacy_json(path.clone());
        match wait_for_project_event(&mut app) {
            super::super::project_state::ProjectEvent::Loaded {
                path: loaded,
                project,
            } => {
                app.apply_loaded_project(loaded, project);
            }
            super::super::project_state::ProjectEvent::Saved(_) => {
                panic!("expected Loaded, got Saved")
            }
            super::super::project_state::ProjectEvent::Failed(error) => {
                panic!("load failed: {error}")
            }
        }
        let _ = std::fs::remove_file(&path);
        assert_eq!(app.document.performers.len(), roster);
        assert_eq!(
            app.document
                .performers
                .iter()
                .map(|performer| performer.label.as_str())
                .collect::<Vec<_>>(),
            labels.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert_eq!(app.document.sets[0].positions.last().copied(), Some(placed));
        assert_eq!(app.current_path.as_deref(), Some(path.as_path()));
        assert!(!app.dirty);
        assert!(!app.playing);
        assert_eq!(app.field_tool, super::super::FieldTool::Select);
        assert!(app.selected.is_empty());
        assert!(app.is_editable_set_start());
    }

    #[test]
    fn create_place_scrub_play_then_save_keeps_committed_sets() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.place_performer_at(Point { x: 8.0, y: 6.0 }, true);
        app.execute_command(Command::DuplicateSet, &context);
        assert_eq!(app.document.sets.len(), 2);
        let committed = app.document.sets[0].positions.clone();
        app.scrub_to(0, 4.0);
        assert!(!app.playing);
        assert!(!app.is_editable_set_start());
        assert_eq!(app.document.sets[0].positions, committed);
        app.playing = true;
        app.execute_command(Command::PlayPause, &context);
        assert!(!app.playing);
        app.return_to_editable_set_start();
        assert!(app.is_editable_set_start());
        app.speed = super::super::PLAYBACK_SPEED_PRESETS[2];

        let path = std::env::temp_dir().join(format!(
            "drillforge-timeline-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        app.project_state
            .save_legacy_json(path.clone(), app.document.clone());
        match wait_for_project_event(&mut app) {
            super::super::project_state::ProjectEvent::Saved(saved) => {
                app.apply_saved_path(saved);
            }
            super::super::project_state::ProjectEvent::Loaded { .. } => {
                panic!("expected Saved, got Loaded")
            }
            super::super::project_state::ProjectEvent::Failed(error) => {
                panic!("save failed: {error}")
            }
        }
        let _ = std::fs::remove_file(&path);
        assert_eq!(app.document.sets.len(), 2);
        assert_eq!(app.document.sets[0].positions, committed);
        assert!(!app.dirty);
    }

    #[test]
    fn inspector_list_and_field_share_the_same_selection() {
        let mut app = DrillApp::default();
        app.begin_new_show();
        assert!(app.selected.is_empty());

        app.select_performer_from_list(3, false);
        assert_eq!(app.selected, [3].into_iter().collect());

        app.select_performer_from_list(5, true);
        assert_eq!(app.selected, [3, 5].into_iter().collect());

        app.select_performer_from_list(3, true);
        assert_eq!(app.selected, [5].into_iter().collect());

        app.replace_selection([0, 2].into_iter().collect());
        assert_eq!(app.selected, [0, 2].into_iter().collect());
        app.clear_selection();
        assert!(app.selected.is_empty());
    }

    #[test]
    fn playback_locks_add_and_remove_with_a_visible_reason() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.playing = true;
        app.selected = [0].into_iter().collect();
        let roster = app.document.performers.len();
        assert!(
            Command::AddPerformer
                .enabled(app.command_context(), app.locale)
                .is_err()
        );
        assert_eq!(
            Command::RemoveSelectedPerformers.enabled(app.command_context(), app.locale),
            Err(super::super::i18n::registered(Locale::Ja, "core-edit.030"))
        );
        app.execute_command(Command::RemoveSelectedPerformers, &context);
        assert_eq!(app.document.performers.len(), roster);
    }

    #[test]
    fn new_document_prompts_when_the_show_is_dirty() {
        let context = egui::Context::default();
        let mut app = DrillApp {
            dirty: true,
            ..DrillApp::default()
        };
        app.execute_command(Command::NewDocument, &context);
        assert!(matches!(
            app.document_open_guard,
            super::super::DocumentOpenGuard::Prompt(super::super::DocumentOpenTarget::NewShow)
        ));
        assert_eq!(
            app.document.sets.len(),
            DrillApp::default().document.sets.len()
        );
    }

    #[test]
    fn new_document_starts_immediately_when_clean() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        let demo_sets = app.document.sets.len();
        assert!(demo_sets > 1);
        app.execute_command(Command::NewDocument, &context);
        assert_eq!(app.document.sets.len(), 1);
        assert_eq!(app.document.performers.len(), 16);
        assert!(matches!(
            app.document_open_guard,
            super::super::DocumentOpenGuard::Idle
        ));
    }

    #[test]
    fn menus_open_every_workspace_and_english_copy_is_clean() {
        let context = egui::Context::default();
        let mut app = DrillApp {
            locale: Locale::En,
            ..DrillApp::default()
        };
        app.selected.insert(0);

        for menu in [
            Menu::Edit,
            Menu::Set,
            Menu::Playback,
            Menu::Workspace,
            Menu::View,
            Menu::Help,
        ] {
            let specs = commands::specs(menu).collect::<Vec<_>>();
            assert!(!specs.is_empty(), "empty desktop menu: {menu:?}");
            for spec in specs {
                assert!(!contains_japanese(spec.command.label(Locale::En)));
            }
        }

        for (command, expected) in [
            (Command::FocusPerformerTools, WorkspaceFocus::Performer),
            (Command::FocusClinic, WorkspaceFocus::Clinic),
            (Command::FocusGrid, WorkspaceFocus::Grid),
            (Command::FocusTempo, WorkspaceFocus::Tempo),
            (Command::FocusVideo, WorkspaceFocus::Video),
            (Command::FocusAudio, WorkspaceFocus::Audio),
        ] {
            app.show_inspector = false;
            app.execute_command(command, &context);
            assert_eq!(app.workspace_focus, Some(expected));
            assert!(
                app.show_inspector,
                "a workspace command must not focus a hidden inspector"
            );
        }
        app.execute_command(Command::OpenPrint, &context);
        assert!(app.print_state.open);
        app.execute_command(Command::OpenProductionSheet, &context);
        assert!(app.production_sheet_workspace.open);
        assert_eq!(app.workspace_focus, Some(WorkspaceFocus::ProductionSheet));
        app.execute_command(Command::GettingStarted, &context);
        assert!(app.onboarding.show_help);
    }

    #[test]
    fn inspector_commits_are_atomic_and_individually_undoable() {
        let mut app = DrillApp::default();
        let original_grid = app.document.grid.clone();
        let original_tempo = app.document.tempo.clone();

        let mut grid = original_grid.clone();
        grid.width += 8.0;
        app.history
            .execute(
                &mut app.document,
                Edit::ReplaceGrid {
                    grid: grid.clone(),
                    scale_positions: false,
                },
            )
            .unwrap();

        let mut tempo = original_tempo.clone();
        tempo.set(16.0, 144.0);
        app.history
            .execute(
                &mut app.document,
                Edit::SetTempoMap {
                    tempo: tempo.clone(),
                },
            )
            .unwrap();

        let audio = drill_core::audio::AudioTrack {
            path: "reference.wav".into(),
            duration_seconds: 30.0,
            gain_db: -3.0,
            ..Default::default()
        };
        app.history
            .execute(
                &mut app.document,
                Edit::SetAudioTrack {
                    audio: Some(audio.clone()),
                },
            )
            .unwrap();

        assert_eq!(app.document.audio.as_ref(), Some(&audio));
        assert!(app.history.undo(&mut app.document));
        assert!(app.document.audio.is_none());
        assert_eq!(app.document.tempo.events(), tempo.events());
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.tempo.events(), original_tempo.events());
        assert_eq!(app.document.grid, grid);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.grid, original_grid);
    }

    #[test]
    fn rejected_inspector_commit_leaves_document_and_history_unchanged() {
        let mut app = DrillApp::default();
        let original = app.document.clone();
        let mut invalid = app.document.grid.clone();
        invalid.width = f32::NAN;

        assert!(
            app.history
                .execute(
                    &mut app.document,
                    Edit::ReplaceGrid {
                        grid: invalid,
                        scale_positions: false,
                    },
                )
                .is_err()
        );
        assert_eq!(app.document, original);
        assert!(!app.history.can_undo());
    }

    #[test]
    fn menu_widgets_render_headlessly_at_supported_scales() {
        for scale in [1.0_f32, 1.5, 2.0] {
            let context = egui::Context::default();
            context.enable_accesskit();
            context.set_pixels_per_point(scale);
            let output = context.run_ui(egui::RawInput::default(), |ui| {
                for menu in [
                    Menu::File,
                    Menu::Edit,
                    Menu::Arrange,
                    Menu::Set,
                    Menu::Playback,
                    Menu::Workspace,
                    Menu::View,
                    Menu::Help,
                ] {
                    let _ = commands::show_menu(
                        ui,
                        menu,
                        commands::Context {
                            can_undo: true,
                            can_redo: true,
                            has_performers: true,
                            has_selection: true,
                            has_recent_selection: true,
                            has_sets: true,
                            has_previous_production_marker: true,
                            has_next_production_marker: true,
                            has_previous_set: true,
                            has_next_set: true,
                            ..commands::Context::default()
                        },
                        Locale::En,
                    );
                }
            });
            assert!(!output.shapes.is_empty(), "no menu shapes at scale {scale}");
            let tree = output
                .platform_output
                .accesskit_update
                .expect("desktop command menus must expose an AccessKit tree");
            for spec in commands::SPECS {
                let expected = spec.command.label(Locale::En);
                let node = tree
                    .nodes
                    .iter()
                    .map(|(_, node)| node)
                    .find(|node| {
                        node.label()
                            .is_some_and(|label| label.starts_with(expected))
                    })
                    .unwrap_or_else(|| panic!("missing accessible command {expected} at {scale}x"));
                assert_eq!(node.role(), egui::accesskit::Role::Button);
                let bounds = node.bounds().expect("command needs screen bounds");
                assert!(bounds.width() > 0.0 && bounds.height() >= 18.0);
            }
        }
    }

    #[test]
    fn disabled_command_accesskit_label_explains_why() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            let _ = commands::show_menu(ui, Menu::Edit, commands::Context::default(), Locale::En);
        });
        let tree = output.platform_output.accesskit_update.unwrap();
        for expected_reason in ["Nothing to undo", "Nothing to redo", "No performers"] {
            assert!(tree.nodes.iter().any(|(_, node)| {
                node.label()
                    .is_some_and(|label| label.contains(expected_reason))
                    && node.is_disabled()
            }));
        }
    }

    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn native_tab_enter_space_and_escape_keyboard_contract() {
        let context = egui::Context::default();
        let render = |raw_input: egui::RawInput| {
            let mut activated = None;
            let output = context.run_ui(raw_input, |ui| {
                if ui.button("First command").clicked() {
                    activated = Some(1);
                }
                if ui.button("Second command").clicked() {
                    activated = Some(2);
                }
            });
            (activated, output)
        };

        let _ = render(egui::RawInput::default());
        let mut tab = egui::RawInput::default();
        tab.events.push(key_event(egui::Key::Tab));
        let _ = render(tab);
        assert!(context.memory(|memory| memory.focused().is_some()));

        let mut enter = egui::RawInput::default();
        enter.events.push(key_event(egui::Key::Enter));
        assert!(
            render(enter).0.is_some(),
            "Enter must activate the focused button"
        );

        let mut space = egui::RawInput::default();
        space.events.push(key_event(egui::Key::Space));
        assert!(
            render(space).0.is_some(),
            "Space must activate the focused button"
        );

        let mut escape = egui::RawInput::default();
        escape.events.push(key_event(egui::Key::Escape));
        let _ = render(escape);
        assert!(context.memory(|memory| memory.focused().is_none()));
    }

    #[test]
    fn formation_composition_tools_are_single_undo_transactions() {
        let mut app = DrillApp {
            selected: (0..16).collect(),
            ..DrillApp::default()
        };
        let original = app.document.clone();

        app.apply_morph_preview(0.5);
        assert_ne!(app.document, original);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document, original);

        app.apply_radial_selection(4);
        assert_ne!(app.document, original);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document, original);

        app.commit_shape(drill_core::shapes::ShapeSpec::Circle {
            center: Point { x: 50.0, y: 42.0 },
            radius: 20.0,
        });
        let shaped = app.document.clone();
        assert!(shaped.sets[0].shape.is_some());
        app.apply_section_shape_assignment();
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document, shaped);
    }

    #[test]
    fn formation_preview_never_mutates_until_single_undoable_apply() {
        let mut app = DrillApp {
            selected: (0..12).collect(),
            ..DrillApp::default()
        };
        let before = app.document.clone();
        app.preview_shape(drill_core::shapes::ShapeSpec::Ellipse {
            center: Point { x: 50.0, y: 42.0 },
            radius_x: 18.0,
            radius_y: 8.0,
            rotation: 0.0,
        });
        assert_eq!(app.document, before);
        assert_eq!(app.formation_preview_points.len(), 12);
        app.apply_shape_preview();
        assert_ne!(app.document, before);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document, before);

        app.begin_free_draw();
        app.free_draw_raw = vec![
            Point { x: 10.0, y: 10.0 },
            Point { x: 20.0, y: 20.0 },
            Point { x: 30.0, y: 10.0 },
        ];
        app.finish_free_draw_preview();
        assert_eq!(app.document, before);
        assert!(matches!(
            app.formation_preview_spec,
            Some(drill_core::shapes::ShapeSpec::FreePath { .. })
        ));
    }

    #[test]
    fn form_first_free_draw_selects_everyone_without_mutating_until_apply() {
        let mut app = DrillApp::default();
        let before = app.document.clone();
        app.begin_free_draw();
        assert_eq!(app.selected.len(), app.document.performers.len());
        assert!(app.free_draw_active);
        assert_eq!(app.document, before);

        app.free_draw_raw = vec![
            Point { x: 12.0, y: 12.0 },
            Point { x: 50.0, y: 42.0 },
            Point { x: 88.0, y: 12.0 },
        ];
        app.finish_free_draw_preview();
        assert_eq!(app.document, before);
        assert_eq!(
            app.formation_preview_points.len(),
            app.document.performers.len()
        );
        app.apply_shape_preview();
        assert_ne!(app.document, before);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document, before);
    }

    #[test]
    fn preview_closure_and_discard_are_non_destructive() {
        let mut app = DrillApp {
            selected: (0..8).collect(),
            ..DrillApp::default()
        };
        let before = app.document.clone();
        app.preview_shape(drill_core::shapes::ShapeSpec::Circle {
            center: Point { x: 50.0, y: 42.0 },
            radius: 16.0,
        });
        assert!(app.formation_preview_is_closed());
        app.cancel_shape_preview();
        assert!(app.formation_preview_points.is_empty());
        assert!(app.formation_preview_spec.is_none());
        assert_eq!(app.document, before);
    }

    /// "Follow the Leader" auto-generates intermediate Sets instead of a
    /// live continuous-path primitive (the Pyware-style version the user
    /// found hard to fine-tune after the fact), so this exercises the same
    /// invariants the design rests on: the right number of ordinary Sets
    /// land right after the current one, performers outside the moving
    /// group hold their position across every inserted set, and the whole
    /// multi-set insertion is a single undoable action.
    #[test]
    fn follow_the_leader_inserts_sets_as_a_single_undo_transaction() {
        let mut app = DrillApp {
            selected: [0, 1, 2].into_iter().collect(),
            ..DrillApp::default()
        };
        let before = app.document.clone();
        let initial_sets = app.document.sets.len();
        let start = app.document.sets[app.current_set].positions[0];
        let end = Point {
            x: (start.x + 20.0).min(app.document.grid.width),
            y: start.y,
        };
        // Performer 3 is not part of the moving group; it must hold its
        // existing position across every inserted set.
        let bystander = 3usize;
        let bystander_pos = app.document.sets[app.current_set].positions[bystander];

        app.apply_follow_the_leader(
            drill_core::shapes::ShapeSpec::FreePath {
                vertices: vec![start, end],
            },
            5,
        );

        assert_eq!(app.document.sets.len(), initial_sets + 5);
        assert_eq!(app.current_set, 1);
        for set in &app.document.sets[1..=5] {
            assert_eq!(set.positions[bystander], bystander_pos);
        }
        // Rank 0 (the leader, performer 0) has zero delay, so it reaches
        // the path's end exactly at the final inserted set.
        let last = &app.document.sets[5];
        assert!((last.positions[0].x - end.x).abs() < 1.0);
        assert!((last.positions[0].y - end.y).abs() < 1.0);
        // Rank 2 (the last follower) lags behind and has not caught up to
        // the leader by the final inserted set.
        assert_ne!(last.positions[2], last.positions[0]);

        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document, before);
    }

    /// A three-performer run over a straight path, left live (not baked).
    fn app_with_follow_the_leader(steps: usize) -> DrillApp {
        let mut app = DrillApp {
            selected: [0, 1, 2].into_iter().collect(),
            ..DrillApp::default()
        };
        let start = app.document.sets[app.current_set].positions[0];
        let end = Point {
            x: (start.x + 20.0).min(app.document.grid.width),
            y: start.y,
        };
        app.apply_follow_the_leader(
            drill_core::shapes::ShapeSpec::FreePath {
                vertices: vec![start, end],
            },
            steps,
        );
        assert_eq!(app.document.generators.len(), 1);
        app
    }

    /// The point of keeping the generator live is that the step count stays
    /// editable, so a re-run must behave like an edit of the existing range
    /// and not like a fresh insertion: only the owned sets change, the sets
    /// around them keep their identity, non-participants keep whatever the
    /// designer put there, and the whole rewrite is one undo away.
    #[test]
    fn regenerating_follow_the_leader_rewrites_only_the_owned_range() {
        let mut app = app_with_follow_the_leader(5);
        let generator_id = app.document.generators[0].id;
        assert_eq!(app.document.generators[0].owns.len(), 5);
        for set in &app.document.sets[1..=5] {
            assert_eq!(set.generated_by, Some(generator_id));
        }
        let trailing = app.document.sets.last().cloned().expect("trailing set");
        let sets_before = app.document.sets.len();

        // Performer 3 is outside the moving group. Written straight into the
        // document so this exercises regeneration alone, not the manual-edit
        // detach path that a real drag would take.
        let bystander = 3usize;
        let marker = Point { x: 5.0, y: 5.0 };
        app.document.sets[2].positions[bystander] = marker;

        app.regenerate_follow_the_leader(generator_id, Some(8), None);
        assert_eq!(app.document.sets.len(), sets_before + 3);
        assert_eq!(app.document.generators.len(), 1);
        assert_eq!(app.document.generators[0].steps, 8);
        assert_eq!(app.document.generators[0].owns.len(), 8);
        for set in &app.document.sets[1..=8] {
            assert_eq!(set.generated_by, Some(generator_id));
        }
        assert_eq!(app.document.sets[2].positions[bystander], marker);
        assert_eq!(app.document.sets.last().expect("trailing set"), &trailing);

        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.sets.len(), sets_before);
        assert_eq!(app.document.generators[0].steps, 5);
        assert_eq!(app.document.sets[2].positions[bystander], marker);

        // Shrinking drops the tail of the range rather than leaving orphans.
        app.regenerate_follow_the_leader(generator_id, Some(2), None);
        assert_eq!(app.document.sets.len(), sets_before - 3);
        assert_eq!(app.document.generators[0].owns.len(), 2);
        assert_eq!(
            app.document
                .sets
                .iter()
                .filter(|set| set.generated_by.is_some())
                .count(),
            2
        );
    }

    /// A hand edit inside a generated range would be silently overwritten by
    /// the next regeneration, so the edit wins and the generator lets go of
    /// the whole range at once.
    #[test]
    fn manual_edit_in_a_generated_set_detaches_the_whole_range() {
        let mut app = app_with_follow_the_leader(4);
        let generator_id = app.document.generators[0].id;
        app.navigate_to_set(2);
        app.selected = [0].into_iter().collect();
        let mut points = app.selected_points();
        let original = points[0];
        points[0] = Point {
            x: points[0].x + 4.0,
            y: points[0].y + 4.0,
        };
        let requested = points[0];
        app.commit_layout(points);

        assert!(app.document.generators.is_empty());
        assert!(app.document.generator(generator_id).is_none());
        assert!(app.document.sets.iter().all(|s| s.generated_by.is_none()));
        // The move still lands where the designer asked, modulo grid snapping.
        let landed = app.document.sets[2].positions[0];
        assert_ne!(landed, original);
        assert!((landed.x - requested.x).abs() < 0.5);
        assert!((landed.y - requested.y).abs() < 0.5);

        // Detach and move are one undoable action, not two.
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document.generators.len(), 1);
        assert_eq!(app.document.sets[2].generated_by, Some(generator_id));
    }

    #[test]
    fn baking_a_generator_drops_the_tag_without_moving_anyone() {
        let mut app = app_with_follow_the_leader(4);
        let generator_id = app.document.generators[0].id;
        let before: Vec<Vec<Point>> = app
            .document
            .sets
            .iter()
            .map(|set| set.positions.clone())
            .collect();

        app.bake_follow_the_leader(generator_id);

        assert!(app.document.generators.is_empty());
        assert!(app.document.sets.iter().all(|s| s.generated_by.is_none()));
        let after: Vec<Vec<Point>> = app
            .document
            .sets
            .iter()
            .map(|set| set.positions.clone())
            .collect();
        assert_eq!(before, after);
    }

    /// Generators are additive optional state, so both directions have to
    /// hold: a document with them survives a save/load, and a document from
    /// before they existed still opens.
    #[test]
    fn generators_round_trip_and_older_documents_still_load() {
        let app = app_with_follow_the_leader(3);
        let json = app.document.to_json().expect("serialize");
        let loaded = drill_core::Document::from_json(&json).expect("round trip");
        assert_eq!(loaded, app.document);
        assert_eq!(loaded.generators.len(), 1);
        assert!(loaded.sets[1].generated_by.is_some());

        let mut legacy: serde_json::Value = serde_json::from_str(&json).expect("parse");
        legacy
            .as_object_mut()
            .expect("document object")
            .remove("generators");
        for set in legacy["sets"].as_array_mut().expect("sets array") {
            set.as_object_mut()
                .expect("set object")
                .remove("generated_by");
        }
        let legacy =
            drill_core::Document::from_json(&serde_json::to_string(&legacy).expect("re-serialize"))
                .expect("older document loads");
        assert!(legacy.generators.is_empty());
        assert!(legacy.sets.iter().all(|set| set.generated_by.is_none()));
    }

    #[test]
    fn escape_command_cancels_preview_before_clearing_selection() {
        let context = egui::Context::default();
        let mut app = DrillApp {
            selected: (0..4).collect(),
            ..DrillApp::default()
        };
        let selected = app.selected.clone();
        app.preview_shape(drill_core::shapes::ShapeSpec::Circle {
            center: Point { x: 50.0, y: 42.0 },
            radius: 12.0,
        });
        app.execute_command(Command::ClearSelection, &context);
        assert!(app.formation_preview_spec.is_none());
        assert_eq!(app.selected, selected);
        app.execute_command(Command::ClearSelection, &context);
        assert!(app.selected.is_empty());
    }

    #[test]
    fn document_mutation_actions_are_edit_routed_by_source_guard() {
        // Loading a project is a lifecycle boundary which intentionally resets
        // history. Every in-document action must go through Edit/History.
        let sources = [
            ("app_state.rs", include_str!("app_state.rs")),
            ("app_ui.rs", include_str!("app_ui.rs")),
            (
                "workspace_inspector.rs",
                include_str!("workspace_inspector.rs"),
            ),
            ("inspector_media.rs", include_str!("inspector_media.rs")),
            ("import_state.rs", include_str!("import_state.rs")),
            ("formation field", include_str!("field_view.rs")),
            ("simple_mode.rs", include_str!("simple_mode.rs")),
        ];
        for (name, source) in sources {
            assert!(
                !source.contains("history.push("),
                "legacy index edit in {name}"
            );
            assert!(
                !source.contains("command.apply(&mut self.document"),
                "direct command mutation in {name}"
            );
            assert!(
                !source.contains("self.document.sets[self.current_set].positions[index] ="),
                "direct position mutation in {name}"
            );
        }
        let ui = include_str!("app_ui.rs");
        assert_eq!(ui.matches("self.document =").count(), 0);
        assert!(include_str!("app_state.rs").contains("self.document = project.document;"));
        assert!(!include_str!("workspace_inspector.rs").contains("self.document ="));
    }

    #[test]
    fn stale_ui_edit_is_atomic_and_valid_edit_round_trips() {
        let mut app = DrillApp::default();
        let before = app.document.clone();
        let performer = app.document.performers[0].id;
        let invalid_set = drill_core::SetId::new(u32::MAX).unwrap();
        assert!(!app.execute_edit(
            Edit::MovePerformers {
                set_id: invalid_set,
                performer_ids: vec![performer],
                positions: vec![Point { x: 1.0, y: 1.0 }],
            },
            "stale",
        ));
        assert_eq!(app.document, before);
        assert!(!app.history.can_undo());

        let set_id = app.document.sets[0].id;
        let target = Point { x: 12.0, y: 13.0 };
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![performer],
                positions: vec![target],
            },
            "valid",
        ));
        assert_eq!(app.document.sets[0].positions[0], target);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document, before);
        assert!(app.history.redo(&mut app.document));
        assert_eq!(app.document.sets[0].positions[0], target);
    }

    #[test]
    fn user_facing_helper_errors_remain_typed_and_locale_bound() {
        let underlay = include_str!("underlay_state.rs");
        let text_export = include_str!("text_export_state.rs");
        for (name, source) in [("underlay", underlay), ("text export", text_export)] {
            assert!(
                !source.contains("Failed(String)"),
                "untyped event in {name}"
            );
            assert!(
                !source.contains("map_err(|e| e.to_string())"),
                "opaque error conversion in {name}"
            );
            assert!(
                !source.contains("Result<(), &'static str>"),
                "string result in {name}"
            );
        }
        assert!(underlay.contains("enum UnderlayFailure"));
        assert!(underlay.contains("fn localized"));
        assert!(text_export.contains("enum TextExportError"));
        assert!(text_export.contains("fn localized"));
        let import = include_str!("import_state.rs");
        assert!(!import.contains("Failed(String)"));
        assert!(import.contains("enum ImportFailure"));
        assert!(import.contains("fn localized"));
        let snapshots = include_str!("subset_snapshot_state.rs");
        assert!(!snapshots.contains("Result<(), String>"));
        assert!(!snapshots.contains("Result<MergePreview, String>"));
        assert!(!snapshots.contains("map_err(|e| e.to_string())"));
        assert!(snapshots.contains("enum SnapshotStateError"));
    }

    #[test]
    fn restore_previous_selection_is_session_only_and_never_touches_undo() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        let document = app.document.clone();
        let revision = app.history.revision();

        app.execute_command(Command::SelectAll, &context);
        let everyone = app.selected.clone();
        app.execute_command(Command::ClearSelection, &context);
        assert!(app.selected.is_empty());
        assert!(app.can_restore_selection());

        app.execute_command(Command::RestoreRecentSelection, &context);
        assert_eq!(app.selected, everyone);
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        assert!(!app.can_restore_selection());
    }

    #[test]
    fn choosing_an_older_working_group_preserves_current_group_and_document() {
        let mut app = DrillApp::default();
        let document = app.document.clone();
        let revision = app.history.revision();

        app.replace_selection([0_usize, 1].into_iter().collect());
        app.replace_selection([2_usize].into_iter().collect());
        app.replace_selection([3_usize].into_iter().collect());
        // History newest-first is [2], [0, 1]; choose the older group.
        app.restore_selection_history_at(1);

        assert_eq!(app.selected, [0_usize, 1].into_iter().collect());
        assert!(
            app.selection_stack
                .contains(&[3_usize].into_iter().collect())
        );
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn replacing_the_document_discards_selection_restore_state() {
        let mut app = DrillApp::default();
        app.replace_selection((0..app.document.performers.len()).collect());
        app.clear_selection();
        assert!(app.can_restore_selection());

        app.reset_selection_for_document();
        assert!(app.selected.is_empty());
        assert!(!app.can_restore_selection());
    }

    #[test]
    fn knife_splits_a_known_selection_by_a_known_cut_line_and_touches_no_history() {
        let mut app = DrillApp::default();

        // Four performers at known positions, two on each side of a
        // straight vertical cut at x = 0.
        app.document.sets[app.current_set].positions[0] = Point { x: -10.0, y: 0.0 };
        app.document.sets[app.current_set].positions[1] = Point { x: -5.0, y: 3.0 };
        app.document.sets[app.current_set].positions[2] = Point { x: 5.0, y: -2.0 };
        app.document.sets[app.current_set].positions[3] = Point { x: 10.0, y: 1.0 };
        app.selected = [0_usize, 1, 2, 3].into_iter().collect();

        // Everything above is test setup, not part of what Knife itself
        // should be judged against: capture the document/history baseline
        // only now, so the assertions below prove the cut itself makes no
        // further document or undo-history change.
        let document_before = app.document.clone();
        let revision = app.history.revision();

        // A vertical line (dx = 0, dy = 40) gives cross = -1 * (px - 0), so
        // side_a (cross >= 0) is exactly the performers with x <= 0.
        app.apply_knife_cut(Point { x: 0.0, y: -20.0 }, Point { x: 0.0, y: 20.0 });

        let result = app.knife_result.clone().expect("cut produced a result");
        assert_eq!(result.side_a, [0_usize, 1].into_iter().collect());
        assert_eq!(result.side_b, [2_usize, 3].into_iter().collect());
        // The lower-indexed performer (0) is on side_a, so side_a is the
        // default active selection.
        assert!(result.active_side_a);
        assert_eq!(app.selected, result.side_a);

        assert_eq!(app.document, document_before);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn knife_invert_swaps_sides_without_touching_selection_history_or_undo() {
        let mut app = DrillApp::default();
        let revision = app.history.revision();
        app.document.sets[app.current_set].positions[0] = Point { x: -1.0, y: 0.0 };
        app.document.sets[app.current_set].positions[1] = Point { x: 1.0, y: 0.0 };
        app.selected = [0_usize, 1].into_iter().collect();

        app.apply_knife_cut(Point { x: 0.0, y: -5.0 }, Point { x: 0.0, y: 5.0 });
        let left = app.selected.clone();
        let stack_depth = app.selection_stack.len();

        app.invert_knife_side();
        assert_ne!(app.selected, left);
        assert_eq!(app.selection_stack.len(), stack_depth);

        app.invert_knife_side();
        assert_eq!(app.selected, left);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn knife_with_empty_selection_cuts_the_whole_cast() {
        let mut app = DrillApp::default();
        app.selected.clear();
        let total = app.document.performers.len();

        app.apply_knife_cut(Point { x: 0.0, y: -1000.0 }, Point { x: 0.0, y: 1000.0 });

        let result = app.knife_result.expect("cut produced a result");
        assert_eq!(result.side_a.len() + result.side_b.len(), total);
    }

    #[test]
    fn knife_ignores_a_degenerate_zero_length_line() {
        let mut app = DrillApp {
            selected: [0_usize, 1].into_iter().collect(),
            ..DrillApp::default()
        };
        let before = app.selected.clone();

        app.apply_knife_cut(Point { x: 3.0, y: 3.0 }, Point { x: 3.0, y: 3.0 });

        assert!(app.knife_result.is_none());
        assert_eq!(app.selected, before);
    }

    #[test]
    fn glue_merges_recent_selections_and_dedupes_without_touching_undo() {
        let mut app = DrillApp::default();
        let document = app.document.clone();
        let revision = app.history.revision();

        app.replace_selection([0_usize, 1].into_iter().collect());
        app.replace_selection([1_usize, 2].into_iter().collect());
        app.replace_selection([3_usize].into_iter().collect());
        // Stack (oldest..newest) is now [{0,1}, {1,2}]; current is {3}.

        app.glue_merge_recent(2);

        assert_eq!(app.selected, [0_usize, 1, 2, 3].into_iter().collect());
        // The two merged entries are consumed; the pre-glue selection ({3})
        // is remembered in their place, so it stays one Restore away.
        assert_eq!(app.selection_stack.len(), 1);
        assert!(
            app.selection_stack
                .contains(&[3_usize].into_iter().collect())
        );
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn glue_merge_one_consumes_only_the_chosen_entry() {
        let mut app = DrillApp::default();
        app.replace_selection([0_usize].into_iter().collect());
        app.replace_selection([1_usize].into_iter().collect());
        app.replace_selection([2_usize].into_iter().collect());
        // Stack (oldest..newest) is now [{0}, {1}]; current is {2}.

        app.glue_merge_one(1); // recency 1 = the older entry, {0}.

        assert_eq!(app.selected, [0_usize, 2].into_iter().collect());
        assert!(
            app.selection_stack
                .contains(&[1_usize].into_iter().collect())
        );
        assert_eq!(app.selection_stack.len(), 2); // {1} plus the pre-glue {2}.
    }

    #[test]
    fn glue_output_can_itself_be_recalled_afterward() {
        let mut app = DrillApp::default();
        app.replace_selection([0_usize].into_iter().collect());
        app.replace_selection([1_usize].into_iter().collect());

        app.glue_merge_one(0); // merges {1} (current) with {0} -> {0, 1}
        assert_eq!(app.selected, [0_usize, 1].into_iter().collect());

        // Moving on to a new selection should push the glued group onto the
        // history stack automatically, the same as any other selection
        // change, without Glue needing its own explicit bookkeeping.
        app.replace_selection([2_usize].into_iter().collect());
        assert!(
            app.selection_stack
                .contains(&[0_usize, 1].into_iter().collect())
        );
    }

    #[test]
    fn undo_and_redo_publish_a_persistent_status_result() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        let before = app.document.clone();
        let mut after = before.clone();
        after.title = "Status strip edit".into();
        app.execute_edit(
            Edit::ReplaceDocument {
                document: Box::new(after),
            },
            "test edit",
        );

        app.execute_command(Command::Undo, &context);
        assert_eq!(app.document, before);
        assert_eq!(app.status, "直前の編集を元に戻しました");
        app.execute_command(Command::Redo, &context);
        assert_eq!(app.status, "編集をやり直しました");
    }

    #[test]
    fn production_marker_navigation_seeks_an_exact_count_without_clearing_selection() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        app.document.production_markers = vec![drill_core::ProductionMarker {
            id: drill_core::ProductionMarkerId::new(1).unwrap(),
            count: 5,
            kind: drill_core::ProductionMarkerKind::Hit,
            label: "Hit".into(),
            detail: String::new(),
        }];
        app.replace_selection([0, 1].into_iter().collect());
        app.count_position = 1.5;
        app.execute_command(Command::NextProductionMarker, &context);

        assert_eq!(
            app.document
                .global_count(app.current_set, app.count_position),
            5.0
        );
        assert_eq!(app.selected, [0, 1].into_iter().collect());
        assert!(!app.playing);
        assert_eq!(app.status, "次のプロダクションマーカーへ移動しました");
    }

    #[test]
    fn set_navigation_seeks_to_the_set_start_without_clearing_selection() {
        let mut app = DrillApp::default();
        app.document.sets[1].name = "Impact".into();
        app.document.sets[0].counts = 8;
        app.replace_selection([0, 1].into_iter().collect());

        app.navigate_to_set(1);

        assert_eq!(app.current_set, 1);
        assert_eq!(app.count_position, 0.0);
        assert_eq!(app.selected, [0, 1].into_iter().collect());
        assert!(!app.playing);
        assert_eq!(app.status, "セットへ移動しました: Impact");
    }

    #[test]
    fn adjacent_set_commands_pause_on_exact_starts_and_respect_boundaries() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        app.document.sets[0].counts = 8;
        app.document.sets[1].counts = 12;
        app.replace_selection([0, 1].into_iter().collect());
        app.playing = true;

        app.execute_command(Command::NextSet, &context);
        assert_eq!(app.current_set, 1);
        assert_eq!(app.count_position, 0.0);
        assert!(!app.playing);
        assert_eq!(app.selected, [0, 1].into_iter().collect());
        assert!(
            Command::NextSet
                .enabled(app.command_context(), app.locale)
                .is_err()
        );

        app.execute_command(Command::PreviousSet, &context);
        assert_eq!(app.current_set, 0);
        assert_eq!(app.count_position, 0.0);
        assert!(
            Command::PreviousSet
                .enabled(app.command_context(), app.locale)
                .is_err()
        );
    }

    #[test]
    fn global_count_navigation_is_integer_paused_and_preserves_selection() {
        let mut app = DrillApp::default();
        app.document.sets[0].counts = 8;
        app.replace_selection([0, 1].into_iter().collect());
        app.playing = true;

        app.navigate_to_global_count(8);

        assert_eq!(app.current_set, 1);
        assert_eq!(app.count_position, 0.0);
        assert!(!app.playing);
        assert_eq!(app.selected, [0, 1].into_iter().collect());
        assert_eq!(app.status, "全体拍へ移動しました: 9");
    }

    #[test]
    fn keyboard_nudge_is_one_undoable_snapped_layout_and_keeps_selection() {
        let mut app = DrillApp::default();
        app.replace_selection([0, 1].into_iter().collect());
        let selected = app.selected.clone();
        let before = app.selected_points();
        let revision = app.history.revision();
        let dx = app.document.grid.horizontal_units / app.document.grid.horizontal_steps as f32;

        app.nudge_selected(1, 0);

        assert_eq!(app.history.revision().0, revision.0 + 1);
        assert_eq!(app.selected, selected);
        assert_eq!(app.status, "選択した演者をグリッド目盛り分移動しました");
        for (old, new) in before.iter().zip(app.selected_points()) {
            assert_eq!(
                new.x,
                app.document
                    .grid
                    .snap(Point {
                        x: old.x + dx,
                        y: old.y
                    })
                    .x
            );
            assert_eq!(new.y, old.y);
        }
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.selected_points(), before);
    }

    #[test]
    fn keyboard_nudge_clamps_to_field_and_empty_selection_is_a_noop() {
        let mut app = DrillApp::default();
        let revision = app.history.revision();
        app.nudge_selected(1, 0);
        assert_eq!(app.history.revision(), revision);

        app.replace_selection([0].into_iter().collect());
        app.document.sets[app.current_set].positions[0] = app.document.grid.snap(Point {
            x: app.document.grid.width,
            y: app.document.grid.height,
        });
        // The snapped boundary point need not sit on an exact multiple of
        // the step size (it doesn't for every grid preset), so a single
        // nudge isn't guaranteed to already be clamped. Take the first
        // nudge as "reach the boundary" and treat its resulting position as
        // the baseline for the actual no-op check below.
        app.nudge_selected(4, 0);
        let point = app.selected_points()[0];
        assert!(point.x <= app.document.grid.width);
        assert!(point.y <= app.document.grid.height);
        assert_eq!(point, app.document.grid.snap(point));
        let clamped_revision = app.history.revision();

        // A further nudge in the same direction cannot move past the field
        // edge, so it must be a no-op: same position, no new undo entry.
        app.nudge_selected(4, 0);
        assert_eq!(app.selected_points()[0], point);
        assert_eq!(app.history.revision(), clamped_revision);
    }

    #[test]
    fn mid_count_preview_never_commits_formation_edits() {
        let mut app = DrillApp::default();
        app.replace_selection([0].into_iter().collect());
        app.count_position = 2.5;
        let document = app.document.clone();
        let revision = app.history.revision();

        app.nudge_selected(1, 0);
        app.commit_designer_positions(
            vec![Point { x: 5.0, y: 5.0 }],
            None,
            "unexpected",
            "unexpected",
        );
        app.begin_free_draw();

        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        assert!(!app.free_draw_active);
        assert_eq!(
            app.status,
            "編集中のフォーメーションはセット開始位置でのみ変更できます。セット開始へ戻ってください。"
        );
    }

    #[test]
    fn return_to_set_start_pauses_and_preserves_the_working_selection() {
        let mut app = DrillApp {
            current_set: 1,
            count_position: 3.5,
            playing: true,
            ..DrillApp::default()
        };
        app.replace_selection([0, 1].into_iter().collect());
        let document = app.document.clone();
        let revision = app.history.revision();

        app.return_to_editable_set_start();

        assert_eq!(app.current_set, 1);
        assert_eq!(app.count_position, 0.0);
        assert!(!app.playing);
        assert!(app.is_editable_set_start());
        assert_eq!(app.selected, [0, 1].into_iter().collect());
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
        assert_eq!(
            app.status,
            "セット開始位置に戻りました。選択はそのままです。"
        );
    }

    #[test]
    fn lock_and_hide_are_stable_id_session_filters_not_document_edits() {
        let mut app = DrillApp::default();
        let document = app.document.clone();
        let revision = app.history.revision();
        let first = app.document.performers[0].id;
        let second = app.document.performers[1].id;

        app.replace_selection([0].into_iter().collect());
        app.lock_selected_performers();
        assert!(app.locked_performers.contains(&first));
        assert!(app.selected.is_empty());
        app.replace_selection([0, 1].into_iter().collect());
        assert_eq!(app.selected, [1].into_iter().collect());
        app.hide_selected_performers();
        assert!(app.hidden_performers.contains(&second));
        assert!(app.selected.is_empty());
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);

        app.clear_performer_filters();
        app.replace_selection([0, 1].into_iter().collect());
        assert_eq!(app.selected, [0, 1].into_iter().collect());
    }

    #[test]
    fn latest_filter_has_a_direct_session_only_recovery() {
        let mut app = DrillApp::default();
        let document = app.document.clone();
        let revision = app.history.revision();
        let first = app.document.performers[0].id;

        app.replace_selection([0].into_iter().collect());
        app.lock_selected_performers();
        assert!(app.locked_performers.contains(&first));
        assert_eq!(app.last_filtered_performers, [first].into_iter().collect());

        assert_eq!(app.restore_last_filtered_performers(), 1);
        assert!(!app.locked_performers.contains(&first));
        assert!(app.last_filtered_performers.is_empty());
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn filtered_performers_can_be_restored_individually_or_by_section() {
        let mut app = DrillApp::default();
        let document = app.document.clone();
        let revision = app.history.revision();
        let first = app.document.performers[0].id;
        let second = app.document.performers[1].id;
        let section = app.document.performers[0].section;

        app.locked_performers.insert(first);
        app.hidden_performers.insert(second);
        app.restore_filtered_performer(first);
        assert!(!app.locked_performers.contains(&first));
        assert!(app.hidden_performers.contains(&second));

        let restored = app.restore_filtered_section(section);
        assert_eq!(restored, 1);
        assert!(app.locked_performers.is_empty());
        assert!(app.hidden_performers.is_empty());
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn restoring_a_selection_respects_filters_added_after_it_was_saved() {
        let mut app = DrillApp::default();
        let first = app.document.performers[0].id;
        app.replace_selection([0, 1].into_iter().collect());
        app.clear_selection();
        app.locked_performers.insert(first);

        app.restore_recent_selection();

        assert_eq!(app.selected, [1].into_iter().collect());
        assert!(app.is_locked_index(0));
    }

    #[test]
    fn visibility_review_skips_session_filtered_performers_and_is_non_destructive() {
        let mut app = DrillApp::default();
        let document = app.document.clone();
        let revision = app.history.revision();
        let locked = app.document.performers[1].id;
        app.locked_performers.insert(locked);

        app.focus_visibility_target(&[0, 1, 2], 1);
        assert_eq!(app.selected, [0].into_iter().collect());
        assert_eq!(app.visibility_focus, Some(app.document.performers[0].id));
        app.focus_visibility_target(&[0, 1, 2], 1);
        assert_eq!(app.selected, [2].into_iter().collect());
        app.focus_visibility_target(&[0, 1, 2], -1);
        assert_eq!(app.selected, [0].into_iter().collect());

        app.select_visibility_targets(&[0, 1, 2]);
        assert_eq!(app.selected, [0, 2].into_iter().collect());
        assert_eq!(app.document, document);
        assert_eq!(app.history.revision(), revision);
    }

    #[test]
    fn set_count_draft_is_non_destructive_until_one_undoable_apply() {
        let mut app = DrillApp::default();
        let before = app.document.clone();
        let old_timeline = app.document.timeline_counts();
        let old_moves = app.document.sets[0].counts;

        app.begin_set_count_draft();
        app.set_count_draft.as_mut().unwrap().moves = old_moves + 4;
        assert_eq!(app.document, before);
        assert_eq!(app.history.revision(), drill_core::Revision(0));

        app.apply_set_count_draft();
        assert_eq!(app.document.sets[0].counts, old_moves + 4);
        assert_eq!(app.document.timeline_counts(), old_timeline + 4);
        assert!(app.set_count_draft.is_none());

        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document, before);
    }

    #[test]
    fn count_draft_is_discarded_if_its_set_no_longer_exists() {
        let mut app = DrillApp::default();
        app.begin_set_count_draft();
        let document = app.document.clone();
        app.set_count_draft.as_mut().unwrap().set_id = drill_core::SetId::new(u32::MAX).unwrap();
        app.apply_set_count_draft();
        assert!(app.set_count_draft.is_none());
        assert_eq!(app.document, document);
    }

    #[test]
    fn timeline_transition_count_rejects_zero_and_commits_a_positive_value() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.execute_command(Command::DuplicateSet, &context);
        let before = app.document.sets[0].counts;
        assert!(!app.commit_transition_counts(0, 0));
        assert_eq!(app.document.sets[0].counts, before);
        assert_eq!(
            app.status,
            super::super::i18n::registered(Locale::Ja, "core-edit.035")
        );
        assert!(app.commit_transition_counts(0, 12));
        assert_eq!(app.document.sets[0].counts, 12);
        assert!(app.history.can_undo());
    }

    #[test]
    fn scrubbing_pauses_playback_and_does_not_commit_mid_set_positions() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.execute_command(Command::DuplicateSet, &context);
        app.navigate_to_set(1);
        app.replace_selection([0].into_iter().collect());
        let origin = app.document.sets[1].positions[0];
        let moved = Point {
            x: (origin.x + 8.0).min(app.document.grid.width),
            y: origin.y,
        };
        let set_id = app.document.sets[1].id;
        let performer_id = app.document.performers[0].id;
        assert!(app.execute_edit(
            Edit::MovePerformers {
                set_id,
                performer_ids: vec![performer_id],
                positions: vec![moved],
            },
            "move for scrub test",
        ));
        let committed_start = app.document.sets[0].positions[0];
        app.playing = true;
        app.scrub_to(0, 4.0);
        assert!(!app.playing);
        assert_eq!(app.current_set, 0);
        assert_eq!(app.count_position, 4.0);
        assert!(!app.is_editable_set_start());
        assert_eq!(app.document.sets[0].positions[0], committed_start);
        assert_eq!(app.document.sets[1].positions[0], moved);
        let mut interpolated = Vec::new();
        app.document
            .positions_at_count(0, app.count_position, &mut interpolated);
        assert_ne!(interpolated[0], committed_start);
        assert_ne!(interpolated[0], moved);
    }

    #[test]
    fn playback_speed_presets_and_jump_to_start_pause_at_count_zero() {
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.speed = 0.5;
        assert!((app.speed - super::super::PLAYBACK_SPEED_PRESETS[0]).abs() < f32::EPSILON);
        app.speed = 2.0;
        assert!((app.speed - super::super::PLAYBACK_SPEED_PRESETS[2]).abs() < f32::EPSILON);
        app.current_set = 0;
        app.count_position = 3.0;
        app.playing = true;
        app.jump_to_show_start();
        assert_eq!(app.current_set, 0);
        assert_eq!(app.count_position, 0.0);
        assert!(!app.playing);
        assert!(app.is_editable_set_start());
    }

    #[test]
    fn set_card_navigation_syncs_the_field_to_the_clicked_set() {
        let context = egui::Context::default();
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.execute_command(Command::DuplicateSet, &context);
        app.document.sets[1].name = "Impact".into();
        app.replace_selection([0, 1].into_iter().collect());
        app.playing = true;
        app.navigate_to_set(1);
        assert_eq!(app.current_set, 1);
        assert_eq!(app.count_position, 0.0);
        assert!(!app.playing);
        assert_eq!(app.selected, [0, 1].into_iter().collect());
        assert!(app.is_editable_set_start());
    }

    #[test]
    fn formation_clipboard_is_previewed_then_applied_as_one_undoable_edit() {
        let mut app = DrillApp::default();
        app.replace_selection([0, 2].into_iter().collect());
        app.copy_selected_formation();
        let before = app.document.clone();
        let copied = app.formation_clipboard.entries.clone();

        app.document.sets[0].positions[0].x += 6.0;
        app.begin_clipboard_paste_preview();
        assert_eq!(
            app.document.sets[0].positions[0].x,
            before.sets[0].positions[0].x + 6.0
        );
        assert_eq!(app.clipboard_paste_preview.as_ref().unwrap(), &copied);

        app.apply_clipboard_paste_preview();
        assert_eq!(
            app.document.sets[0].positions[0],
            before.sets[0].positions[0]
        );
        assert!(app.history.can_undo());
        assert!(app.history.undo(&mut app.document));
        assert_ne!(app.document, before);
    }

    #[test]
    fn formation_clipboard_preview_rejects_mid_count_and_escape_is_non_destructive() {
        let mut app = DrillApp::default();
        app.replace_selection([0].into_iter().collect());
        app.copy_selected_formation();
        app.count_position = 1.0;
        app.begin_clipboard_paste_preview();
        assert!(app.clipboard_paste_preview.is_none());
        app.count_position = 0.0;
        app.begin_clipboard_paste_preview();
        let before = app.document.clone();
        app.cancel_clipboard_paste_preview();
        assert!(app.clipboard_paste_preview.is_none());
        assert_eq!(app.document, before);
    }

    #[test]
    fn formation_clipboard_transplants_shape_onto_equal_size_selected_group() {
        let mut app = DrillApp::default();
        app.document.sets[0].positions[0] = Point { x: 4.0, y: 8.0 };
        app.document.sets[0].positions[2] = Point { x: 12.0, y: 16.0 };
        app.replace_selection([0, 2].into_iter().collect());
        app.copy_selected_formation();

        app.document.sets[0].positions[4] = Point { x: 40.0, y: 24.0 };
        app.document.sets[0].positions[6] = Point { x: 56.0, y: 24.0 };
        let before = app.document.clone();
        app.replace_selection([4, 6].into_iter().collect());
        app.begin_clipboard_paste_preview();

        assert!(app.clipboard_paste_targets_selection);
        let preview = app.clipboard_paste_preview.as_ref().unwrap();
        assert_eq!(preview.len(), 2);
        assert!(preview.iter().all(|(id, _)| {
            *id == app.document.performers[4].id || *id == app.document.performers[6].id
        }));
        let preview_centre = Point {
            x: preview.iter().map(|(_, point)| point.x).sum::<f32>() / 2.0,
            y: preview.iter().map(|(_, point)| point.y).sum::<f32>() / 2.0,
        };
        assert_eq!(preview_centre, Point { x: 48.0, y: 24.0 });

        app.apply_clipboard_paste_preview();
        assert!(!app.clipboard_paste_targets_selection);
        assert_ne!(app.document, before);
        assert!(app.history.undo(&mut app.document));
        assert_eq!(app.document, before);
    }

    /// The marking menu is direction-resolved, so this is the whole contract
    /// that makes "flick and release before the wheel appears" land the same
    /// command as a slow, deliberate pick.
    #[test]
    fn marking_menu_resolves_screen_vectors_to_compass_slices() {
        use super::super::marking_menu::{self, Direction};

        // Screen y grows downward, so a negative dy is north.
        for (dx, dy, expected) in [
            (60.0, 0.0, Direction::E),
            (60.0, -60.0, Direction::Ne),
            (0.0, -60.0, Direction::N),
            (-60.0, -60.0, Direction::Nw),
            (-60.0, 0.0, Direction::W),
            (-60.0, 60.0, Direction::Sw),
            (0.0, 60.0, Direction::S),
            (60.0, 60.0, Direction::Se),
        ] {
            assert_eq!(
                marking_menu::resolve(dx, dy, marking_menu::DEAD_ZONE),
                Some(expected),
                "vector ({dx}, {dy})"
            );
        }
        // Just past a boundary snaps to the neighbouring slice, and distance
        // is irrelevant once the dead zone is cleared.
        assert_eq!(
            marking_menu::resolve(600.0, -260.0, marking_menu::DEAD_ZONE),
            Some(Direction::Ne)
        );
        assert_eq!(
            marking_menu::resolve(600.0, -240.0, marking_menu::DEAD_ZONE),
            Some(Direction::E)
        );
    }

    #[test]
    fn marking_menu_cancels_inside_the_dead_zone() {
        use super::super::marking_menu;

        assert_eq!(
            marking_menu::resolve(0.0, 0.0, marking_menu::DEAD_ZONE),
            None
        );
        assert_eq!(
            marking_menu::resolve(marking_menu::DEAD_ZONE - 1.0, 0.0, marking_menu::DEAD_ZONE),
            None
        );
        assert!(
            marking_menu::resolve(marking_menu::DEAD_ZONE + 1.0, 0.0, marking_menu::DEAD_ZONE)
                .is_some()
        );
        assert_eq!(
            marking_menu::resolve(f32::NAN, 0.0, marking_menu::DEAD_ZONE),
            None
        );
        assert_eq!(
            marking_menu::resolve_sub(0.0, 0.0, marking_menu::SUB_DEAD_ZONE),
            None
        );
    }

    /// Eight slices plus a nested pair must cover the nine field commands
    /// exactly once each: no command lost, none reachable two ways.
    #[test]
    fn marking_menu_slices_cover_every_command_once() {
        use super::super::marking_menu::{self, Direction, MarkingAction, Slice};

        let mut reached = Vec::new();
        let mut sub_menus = 0;
        for direction in Direction::ALL {
            match marking_menu::slice_for(direction) {
                Slice::Action(action) => reached.push(action),
                Slice::SubMenu => sub_menus += 1,
            }
        }
        assert_eq!(sub_menus, 1);
        for (_, action) in marking_menu::SUB_ITEMS {
            reached.push(action);
        }
        for action in [
            MarkingAction::AlignHorizontal,
            MarkingAction::AlignVertical,
            MarkingAction::DistributeHorizontal,
            MarkingAction::DistributeVertical,
            MarkingAction::Straighten,
            MarkingAction::CopyFormation,
            MarkingAction::PasteFormation,
            MarkingAction::Lock,
            MarkingAction::Hide,
        ] {
            assert_eq!(
                reached.iter().filter(|&&reached| reached == action).count(),
                1,
                "{action:?}"
            );
        }
        assert_eq!(reached.len(), 9);
    }

    /// The nested wheel holds two items 90 degrees apart, so its hit regions
    /// are the two half-planes either side of their bisector.
    #[test]
    fn marking_menu_sub_wheel_splits_on_the_bisector() {
        use super::super::marking_menu::{self, MarkingAction};

        let dead = marking_menu::SUB_DEAD_ZONE;
        assert_eq!(
            marking_menu::resolve_sub(-40.0, 0.0, dead),
            Some(MarkingAction::Lock)
        );
        assert_eq!(
            marking_menu::resolve_sub(0.0, -40.0, dead),
            Some(MarkingAction::Hide)
        );
        // Continuing straight out along the north-west opening axis stays on
        // the Lock side rather than flickering between the two.
        assert_eq!(
            marking_menu::resolve_sub(-40.0, -39.0, dead),
            Some(MarkingAction::Lock)
        );
        assert_eq!(
            marking_menu::resolve_sub(40.0, 40.0, dead),
            Some(MarkingAction::Hide)
        );
    }

    #[test]
    fn inspector_single_select_commits_label_section_and_position() {
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.replace_selection([0].into_iter().collect());
        app.ensure_performer_draft();
        let performer_id = app.document.performers[0].id;
        let trumpet = drill_core::Section {
            id: drill_core::SectionId::new(2).expect("section id"),
            name: "Trumpet".into(),
            short: "Tpt".into(),
            color: [64, 180, 255],
            order: 1,
        };
        let trumpet_id = trumpet.id;
        assert!(app.execute_edit(
            Edit::AddSection {
                section: trumpet,
                at: None,
            },
            "add section",
        ));
        {
            let draft = app.performer_draft.as_mut().expect("single-select draft");
            draft.number = "12".into();
            draft.name = "Solo".into();
            draft.label_dirty = true;
            draft.section = trumpet_id;
            draft.section_dirty = true;
        }
        app.commit_performer_draft();
        assert_eq!(app.document.performers[0].label, "12 Solo");
        assert_eq!(app.document.performers[0].id, performer_id);
        assert_eq!(app.document.performers[0].section, trumpet_id);

        app.ensure_performer_draft();
        let target = super::super::controller::field_point(
            Point { x: 9.0, y: 4.0 },
            &app.document,
            app.document.grid.snap_enabled,
        );
        if let Some(draft) = app.performer_draft.as_mut() {
            draft.x = 9.0;
            draft.y = 4.0;
            draft.position_dirty = true;
        }
        app.commit_performer_draft();
        assert_eq!(app.document.sets[0].positions[0], target);
        assert!(app.history.can_undo());
    }

    #[test]
    fn inspector_multi_select_edits_common_section_and_hides_position() {
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.replace_selection([0, 1].into_iter().collect());
        app.ensure_performer_draft();
        let draft = app.performer_draft.as_ref().expect("multi draft");
        assert_eq!(draft.selected.len(), 2);
        assert!(draft.number.is_empty());
        assert_eq!(draft.x, 0.0);
        assert_eq!(draft.y, 0.0);
        let trumpet = drill_core::Section {
            id: drill_core::SectionId::new(2).expect("section id"),
            name: "Trumpet".into(),
            short: "Tpt".into(),
            color: [64, 180, 255],
            order: 1,
        };
        let trumpet_id = trumpet.id;
        assert!(app.execute_edit(
            Edit::AddSection {
                section: trumpet,
                at: None,
            },
            "add section",
        ));
        app.commit_inspector_section(trumpet_id);
        assert_eq!(app.document.performers[0].section, trumpet_id);
        assert_eq!(app.document.performers[1].section, trumpet_id);
        assert_ne!(
            app.document.sets[0].positions[0],
            app.document.sets[0].positions[1]
        );
    }

    #[test]
    fn inspector_commits_then_switches_when_selection_changes() {
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.replace_selection([0].into_iter().collect());
        app.ensure_performer_draft();
        if let Some(draft) = app.performer_draft.as_mut() {
            draft.number = "Q".into();
            draft.name.clear();
            draft.label_dirty = true;
        }
        app.replace_selection([1].into_iter().collect());
        assert_eq!(app.document.performers[0].label, "Q");
        app.ensure_performer_draft();
        assert_eq!(
            app.performer_draft
                .as_ref()
                .map(|draft| draft.selected.clone()),
            Some([1].into_iter().collect())
        );
        app.clear_selection();
        assert!(app.selected.is_empty());
        assert!(app.performer_draft.is_none());
    }

    #[test]
    fn inspector_position_tracks_field_drag_preview() {
        let mut app = DrillApp::default();
        app.begin_new_show();
        app.replace_selection([0].into_iter().collect());
        let start = app.document.sets[0].positions[0];
        app.begin_field_drag(eframe::egui::Pos2::new(0.0, 0.0));
        app.update_field_drag(eframe::egui::Pos2::new(40.0, 0.0), 10.0, true);
        let live = app.inspector_point(0).expect("live point");
        let expected =
            super::super::controller::drag_point(start, (40.0, 0.0), 10.0, &app.document, true);
        assert_eq!(live, expected);
        assert_eq!(app.document.sets[0].positions[0], start);
        app.commit_field_drag();
        assert_eq!(app.document.sets[0].positions[0], expected);
        app.ensure_performer_draft();
        let draft = app.performer_draft.as_ref().expect("synced draft");
        assert!((draft.x - expected.x).abs() < f32::EPSILON);
        assert!((draft.y - expected.y).abs() < f32::EPSILON);
    }
}
