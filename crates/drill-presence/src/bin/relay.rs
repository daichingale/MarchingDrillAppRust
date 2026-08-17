//! The DrillForge presence relay executable.
//!
//! All logic lives in `drill_presence::relay` so tests exercise it directly.
//!
//! Usage: `drill-presence-relay [bind-address]` (default `0.0.0.0:8787`).

use std::net::TcpListener;

fn main() {
    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| drill_presence::relay::DEFAULT_BIND.to_owned());

    let listener = match TcpListener::bind(&bind) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("presence relay: cannot bind {bind}: {error}");
            std::process::exit(1);
        }
    };
    println!("presence relay listening on ws://{bind}/room/<code>");
    drill_presence::relay::serve(&listener);
}
