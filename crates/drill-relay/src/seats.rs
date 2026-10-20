//! Who may edit a room.
//!
//! Viewers still receive the show. Their snapshots and drags are not
//! forwarded, so a full room cannot be overwritten by someone watching.

use drill_sync::{ReplicaId, RoomEvent};
use std::collections::HashMap;

struct Member {
    conn: u64,
    replica: ReplicaId,
    editor: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Direct {
    pub conn: u64,
    pub event: RoomEvent,
}

#[derive(Default)]
pub(crate) struct Decision {
    pub relay: bool,
    pub direct: Vec<Direct>,
}

pub(crate) struct Seats {
    limit: u16,
    rooms: HashMap<String, Vec<Member>>,
}

impl Seats {
    pub(crate) fn new(limit: u16) -> Self {
        Self {
            limit: limit.max(1),
            rooms: HashMap::new(),
        }
    }

    pub(crate) fn observe(&mut self, room: &str, conn: u64, event: &RoomEvent) -> Decision {
        match event {
            RoomEvent::Hello { from, .. } => self.admit(room, conn, *from),
            RoomEvent::Op { from, .. } => Decision {
                relay: self.is_editor(room, *from),
                direct: Vec::new(),
            },
            RoomEvent::Leave { from } => {
                let mut decision = self.release_replica(room, *from);
                decision.relay = true;
                decision
            }
            RoomEvent::Seat { .. } => Decision::default(),
        }
    }

    pub(crate) fn disconnect(&mut self, room: &str, conn: u64) -> Decision {
        self.release_conn(room, conn)
    }

    fn admit(&mut self, room: &str, conn: u64, replica: ReplicaId) -> Decision {
        let limit = self.limit;
        let members = self.rooms.entry(room.to_owned()).or_default();
        if let Some(index) = members.iter().position(|member| member.replica == replica) {
            members[index].conn = conn;
            let editor = members[index].editor;
            let event = seat(replica, editor, members, limit);
            return Decision {
                relay: editor,
                direct: vec![Direct { conn, event }],
            };
        }
        let editors = members.iter().filter(|member| member.editor).count();
        let editor = editors < usize::from(limit);
        members.push(Member {
            conn,
            replica,
            editor,
        });
        let event = seat(replica, editor, members, limit);
        Decision {
            relay: editor,
            direct: vec![Direct { conn, event }],
        }
    }

    fn is_editor(&self, room: &str, replica: ReplicaId) -> bool {
        self.rooms.get(room).is_some_and(|members| {
            members
                .iter()
                .any(|member| member.replica == replica && member.editor)
        })
    }

    fn release_replica(&mut self, room: &str, replica: ReplicaId) -> Decision {
        let Some(members) = self.rooms.get_mut(room) else {
            return Decision::default();
        };
        let Some(index) = members.iter().position(|member| member.replica == replica) else {
            return Decision::default();
        };
        let removed = members.remove(index);
        self.promote(room, removed.editor)
    }

    fn release_conn(&mut self, room: &str, conn: u64) -> Decision {
        let Some(members) = self.rooms.get_mut(room) else {
            return Decision::default();
        };
        let Some(index) = members.iter().position(|member| member.conn == conn) else {
            return Decision::default();
        };
        let removed = members.remove(index);
        self.promote(room, removed.editor)
    }

    fn promote(&mut self, room: &str, was_editor: bool) -> Decision {
        if !was_editor {
            self.drop_empty(room);
            return Decision::default();
        }
        let limit = self.limit;
        let promoted = self.rooms.get_mut(room).and_then(|members| {
            let index = members.iter().position(|member| !member.editor)?;
            members[index].editor = true;
            let replica = members[index].replica;
            let conn = members[index].conn;
            let event = seat(replica, true, members, limit);
            Some(Direct { conn, event })
        });
        if promoted.is_none() {
            self.drop_empty(room);
        }
        Decision {
            relay: false,
            direct: promoted.into_iter().collect(),
        }
    }

    fn drop_empty(&mut self, room: &str) {
        if self.rooms.get(room).is_some_and(Vec::is_empty) {
            self.rooms.remove(room);
        }
    }
}

fn seat(replica: ReplicaId, editor: bool, members: &[Member], limit: u16) -> RoomEvent {
    let editors = members.iter().filter(|member| member.editor).count() as u16;
    RoomEvent::Seat {
        replica,
        editor,
        editors,
        limit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_person_watches_until_an_editor_leaves() {
        let mut seats = Seats::new(1);
        let room = "band";
        let first = seats.observe(
            room,
            1,
            &RoomEvent::Hello {
                from: ReplicaId::from_raw(10),
                name: "A".into(),
                color: [0, 0, 0],
                state: empty_show(10),
            },
        );
        assert!(first.relay);
        assert!(matches!(
            first.direct[0].event,
            RoomEvent::Seat { editor: true, .. }
        ));
        let second = seats.observe(
            room,
            2,
            &RoomEvent::Hello {
                from: ReplicaId::from_raw(11),
                name: "B".into(),
                color: [0, 0, 0],
                state: empty_show(11),
            },
        );
        assert!(!second.relay);
        assert!(matches!(
            second.direct[0].event,
            RoomEvent::Seat {
                editor: false,
                limit: 1,
                ..
            }
        ));
        let op = RoomEvent::Op {
            from: ReplicaId::from_raw(11),
            op: dummy_op(),
        };
        assert!(!seats.observe(room, 2, &op).relay);
        let editor_op = RoomEvent::Op {
            from: ReplicaId::from_raw(10),
            op: dummy_op(),
        };
        assert!(seats.observe(room, 1, &editor_op).relay);
        let promoted = seats.disconnect(room, 1);
        assert!(matches!(
            promoted.direct[0].event,
            RoomEvent::Seat { editor: true, .. }
        ));
        assert_eq!(promoted.direct[0].conn, 2);
    }

    fn empty_show(replica: u64) -> drill_sync::ShowState {
        let document = drill_core::Document::demo(1, 1);
        drill_sync::ShowState::from_document(&document, ReplicaId::from_raw(replica))
    }

    fn dummy_op() -> drill_sync::SyncOp {
        drill_sync::SyncOp::MoveDot {
            dot: drill_sync::DotId {
                set_id: 1,
                performer_id: 1,
            },
            point: drill_core::Point { x: 1.0, y: 2.0 },
            stamp: drill_sync::Stamp {
                lamport: 1,
                replica: ReplicaId::from_raw(11),
            },
        }
    }
}
