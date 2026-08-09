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
            app.execute_command(command, &context);
            assert_eq!(app.workspace_focus, Some(expected));
        }
        app.execute_command(Command::OpenPrint, &context);
        assert!(app.print_state.open);
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
                    Menu::Edit,
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
                            has_sets: true,
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
        assert_eq!(ui.matches("self.document =").count(), 1);
        assert!(ui.contains("self.document = project.document;"));
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
}
