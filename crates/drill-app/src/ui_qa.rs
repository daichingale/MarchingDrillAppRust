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
        let mut app = DrillApp::default();
        app.selected = [0_usize, 1].into_iter().collect();
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
        assert!(app.selection_stack.contains(&[1_usize].into_iter().collect()));
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
}
