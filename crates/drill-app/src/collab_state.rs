//! Document sync beside presence.
//!
//! Presence stays view-only. This module is the one that turns a remote
//! `MoveDot` into an [`Edit`], and the one that publishes a local edit into
//! the room. Remote applies go through `History` so they undo like any other
//! edit, and they are marked so they are not published straight back.

use super::presence_state::SessionSignal;
use drill_core::{Document, Edit, Point};
use drill_sync::{DotId, ReplicaId, RoomEvent, SetFact, ShowState, SyncClient};
use std::time::{Duration, Instant};

pub(crate) enum Echo {
    Moves {
        set_id: u32,
        performer_ids: Vec<u32>,
        points: Vec<Point>,
    },
    Counts {
        set_id: u32,
        counts: u16,
        name: String,
    },
}

pub(crate) fn echo_from(document: &Document, edit: &Edit) -> Option<Echo> {
    match edit {
        Edit::MovePerformers {
            set_id,
            performer_ids,
            positions,
        } => Some(Echo::Moves {
            set_id: set_id.get(),
            performer_ids: performer_ids.iter().map(|id| id.get()).collect(),
            points: positions.clone(),
        }),
        Edit::SetCounts { set_id, counts } => {
            let name = document
                .sets
                .iter()
                .find(|set| set.id == *set_id)
                .map(|set| set.name.clone())
                .unwrap_or_default();
            Some(Echo::Counts {
                set_id: set_id.get(),
                counts: counts.moves,
                name,
            })
        }
        _ => None,
    }
}

#[derive(Default)]
pub(crate) struct CollabState {
    client: Option<SyncClient>,
    show: Option<ShowState>,
    applying: bool,
    room: Option<String>,
    name: String,
    color: [u8; 3],
    last_hello: Option<Instant>,
}

impl CollabState {
    pub(crate) fn is_applying(&self) -> bool {
        self.applying
    }

    pub(crate) fn set_applying(&mut self, applying: bool) {
        self.applying = applying;
    }

    pub(crate) fn is_live(&self) -> bool {
        self.client.is_some()
    }

    pub(crate) fn on_signal(&mut self, signal: &SessionSignal, document: &Document) {
        match signal {
            SessionSignal::Leave => {
                self.client = None;
            }
            SessionSignal::Join {
                host,
                room,
                replica,
                name,
                color,
            } => {
                if self.room.as_deref() != Some(room.as_str()) || self.show.is_none() {
                    self.show = Some(ShowState::from_document(
                        document,
                        ReplicaId::from_raw(*replica),
                    ));
                }
                self.room = Some(room.clone());
                self.name = name.clone();
                self.color = *color;
                let Some(show) = self.show.as_ref() else {
                    return;
                };
                let hello = show.hello(name, *color);
                self.client = Some(SyncClient::connect(host, room, hello));
                self.last_hello = Some(Instant::now());
            }
        }
    }

    pub(crate) fn publish_echo(&mut self, echo: Echo) {
        if self.applying || self.show.is_none() {
            return;
        }
        let Some(show) = self.show.as_mut() else {
            return;
        };
        let from = show.replica();
        let events = match echo {
            Echo::Moves {
                set_id,
                performer_ids,
                points,
            } => performer_ids
                .into_iter()
                .zip(points)
                .filter_map(|(performer_id, point)| {
                    let op = show.move_dot(
                        DotId {
                            set_id,
                            performer_id,
                        },
                        point,
                    )?;
                    Some(RoomEvent::Op { from, op })
                })
                .collect::<Vec<_>>(),
            Echo::Counts {
                set_id,
                counts,
                name,
            } => {
                let op = show.set_meta(
                    set_id,
                    SetFact {
                        name,
                        counts,
                        present: true,
                    },
                );
                vec![RoomEvent::Op { from, op }]
            }
        };
        if let Some(client) = &self.client {
            for event in events {
                client.publish(event);
            }
        }
    }

    /// Drains the socket and returns the edits required to catch the document
    /// up. Also restates the local snapshot every couple of seconds so a
    /// phone that opens the link late still receives the show. The relay
    /// keeps no history.
    pub(crate) fn poll_edits(&mut self, document: &Document) -> Vec<Edit> {
        self.announce_if_due();
        let mut events = Vec::new();
        if let Some(client) = self.client.as_mut() {
            while let Some(event) = client.poll() {
                events.push(event);
            }
        }
        if let Some(show) = self.show.as_mut() {
            for event in &events {
                show.ingest(event);
            }
        }
        self.show
            .as_ref()
            .map(|show| show.edits_against(document))
            .unwrap_or_default()
    }

    fn announce_if_due(&mut self) {
        let due = self
            .last_hello
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(2));
        if !due || self.client.is_none() {
            return;
        }
        let Some(show) = self.show.as_ref() else {
            return;
        };
        let hello = show.hello(&self.name, self.color);
        if let Some(client) = &self.client {
            client.publish(hello);
        }
        self.last_hello = Some(Instant::now());
    }
}
