//! `drill-relay [bind] [--tls cert.pem key.pem]` (default `0.0.0.0:8787`).
//!
//! Replaces `drill-presence-relay` when the room should carry edits as well
//! as cursors. Both speak `/room/<code>`. `--tls` wraps the listener in
//! rustls so phones and PCs can use `wss://` without a second proxy.
//! `DRILLFORGE_TLS_CERT` and `DRILLFORGE_TLS_KEY` are the same pair.

use std::net::TcpListener;

fn main() {
    let mut bind = None;
    let mut cert = std::env::var("DRILLFORGE_TLS_CERT").ok();
    let mut key = std::env::var("DRILLFORGE_TLS_KEY").ok();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--tls" {
            cert = args.next().or(cert);
            key = args.next().or(key);
        } else if bind.is_none() {
            bind = Some(arg);
        } else {
            eprintln!("drill relay: unexpected argument {arg}");
            std::process::exit(2);
        }
    }
    let bind = bind.unwrap_or_else(|| drill_relay::DEFAULT_BIND.to_owned());
    let listener = match TcpListener::bind(&bind) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("drill relay: cannot bind {bind}: {error}");
            std::process::exit(1);
        }
    };
    match (cert, key) {
        (Some(cert_path), Some(key_path)) => {
            let cert_pem = match std::fs::read(&cert_path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    eprintln!("drill relay: cannot read {cert_path}: {error}");
                    std::process::exit(1);
                }
            };
            let key_pem = match std::fs::read(&key_path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    eprintln!("drill relay: cannot read {key_path}: {error}");
                    std::process::exit(1);
                }
            };
            println!("drill relay listening on wss://{bind}/room/<code>");
            if let Err(error) = drill_relay::serve_tls(&listener, &cert_pem, &key_pem) {
                eprintln!("drill relay: {error}");
                std::process::exit(1);
            }
        }
        (None, None) => {
            println!("drill relay listening on ws://{bind}/room/<code>");
            drill_relay::serve(&listener);
        }
        _ => {
            eprintln!("drill relay: pass both a certificate and a private key");
            std::process::exit(2);
        }
    }
}
