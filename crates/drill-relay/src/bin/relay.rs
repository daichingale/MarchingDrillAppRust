//! `drill-relay [bind]` (default `0.0.0.0:8787`).
//!
//! Replaces `drill-presence-relay` when the room should carry edits as well
//! as cursors. Both speak `/room/<code>`.

use std::net::TcpListener;

fn main() {
    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| drill_relay::DEFAULT_BIND.to_owned());
    let listener = match TcpListener::bind(&bind) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("drill relay: cannot bind {bind}: {error}");
            std::process::exit(1);
        }
    };
    println!("drill relay listening on ws://{bind}/room/<code>");
    drill_relay::serve(&listener);
}
