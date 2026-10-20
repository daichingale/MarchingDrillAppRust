//! Browser socket for the same [`drill_sync::RoomEvent`] values the desktop
//! sends. The page's WebSocket stack speaks `wss://` when the page is https.

use drill_presence::PresenceMessage;
use drill_sync::{CoalescedOutbox, RoomEvent};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_time::Instant;

const INBOUND_CAP: usize = 512;

pub struct BrowserClient {
    socket: web_sys::WebSocket,
    inbound: Rc<RefCell<VecDeque<RoomEvent>>>,
    presence: Rc<RefCell<VecDeque<PresenceMessage>>>,
    outbox: RefCell<CoalescedOutbox>,
    open: Rc<Cell<bool>>,
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _on_open: Closure<dyn FnMut(web_sys::Event)>,
}

impl BrowserClient {
    pub fn connect(host: &str, room: &str, hello: RoomEvent) -> Option<Self> {
        let url = drill_sync::socket_url(host, room, page_is_https(host));
        let socket = web_sys::WebSocket::new(&url).ok()?;
        let inbound = Rc::new(RefCell::new(VecDeque::new()));
        let presence = Rc::new(RefCell::new(VecDeque::new()));
        let queue = Rc::clone(&inbound);
        let presence_queue = Rc::clone(&presence);
        let on_message = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
            let Some(text) = event.data().as_string() else {
                return;
            };
            if let Ok(parsed) = RoomEvent::from_json(&text) {
                let mut queue = queue.borrow_mut();
                queue.push_back(parsed);
                while queue.len() > INBOUND_CAP {
                    queue.pop_front();
                }
                return;
            }
            if let Ok(message) = PresenceMessage::from_json(&text) {
                let mut queue = presence_queue.borrow_mut();
                queue.push_back(message);
                while queue.len() > INBOUND_CAP {
                    queue.pop_front();
                }
            }
        }) as Box<dyn FnMut(web_sys::MessageEvent)>);
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        let open = Rc::new(Cell::new(false));
        let flag = Rc::clone(&open);
        let on_open = Closure::wrap(Box::new(move |_event: web_sys::Event| {
            flag.set(true);
        }) as Box<dyn FnMut(web_sys::Event)>);
        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        let mut outbox = CoalescedOutbox::default();
        outbox.push(hello);
        Some(Self {
            socket,
            inbound,
            presence,
            outbox: RefCell::new(outbox),
            open,
            _on_message: on_message,
            _on_open: on_open,
        })
    }

    pub fn publish(&self, event: RoomEvent) {
        self.outbox.borrow_mut().push(event);
        self.flush();
    }

    pub fn poll(&mut self) -> Option<RoomEvent> {
        self.flush();
        self.inbound.borrow_mut().pop_front()
    }

    pub fn poll_presence(&mut self) -> Option<PresenceMessage> {
        self.flush();
        self.presence.borrow_mut().pop_front()
    }

    pub fn send_text(&self, text: &str) {
        if !self.open.get() || self.socket.ready_state() != web_sys::WebSocket::OPEN {
            return;
        }
        let _ = self.socket.send_with_str(text);
    }

    fn flush(&self) {
        if !self.open.get() || self.socket.ready_state() != web_sys::WebSocket::OPEN {
            return;
        }
        let now = Instant::now();
        let mut outbox = self.outbox.borrow_mut();
        while let Some(event) = outbox.pop_ready(now) {
            let Ok(json) = event.to_json() else {
                continue;
            };
            if self.socket.send_with_str(&json).is_err() {
                outbox.unsend(event);
                break;
            }
        }
    }
}

fn page_is_https(host: &str) -> bool {
    if host.starts_with("wss://") || host.starts_with("https://") {
        return true;
    }
    if host.starts_with("ws://") || host.starts_with("http://") {
        return false;
    }
    web_sys::window()
        .and_then(|window| window.location().protocol().ok())
        .is_some_and(|protocol| protocol == "https:")
}
