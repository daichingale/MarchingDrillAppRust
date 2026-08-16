use drill_core::{Document, Edit, History, SetCounts};

#[test]
fn every_count_edit_round_trips_through_history() {
    for moves in [1, 2, 8, 32, 128, 4_096] {
        for hold in [0, 1, 8, 64] {
            if u32::from(moves) + u32::from(hold) > drill_core::transition::MAX_SET_COUNTS {
                continue;
            }
            let mut document = Document::demo(8, 3);
            let original = document.to_json().expect("serialize original");
            let set_id = document.sets[1].id;
            let was_noop = document.sets[1].counts == moves && document.sets[1].hold == hold;
            let mut history = History::with_limit(64);
            history
                .execute(
                    &mut document,
                    Edit::SetCounts {
                        set_id,
                        counts: SetCounts { moves, hold },
                    },
                )
                .unwrap_or_else(|error| {
                    panic!("valid count edit moves={moves} hold={hold}: {error:?}")
                });
            if was_noop {
                assert!(!history.undo(&mut document));
                assert_eq!(document.to_json().expect("serialize no-op"), original);
                continue;
            }
            assert!(
                history.undo(&mut document),
                "undo moves={moves} hold={hold}"
            );
            assert_eq!(document.to_json().expect("serialize undo"), original);
            assert!(history.redo(&mut document));
            assert_eq!(document.sets[1].counts, moves);
            assert_eq!(document.sets[1].hold, hold);
        }
    }
}
