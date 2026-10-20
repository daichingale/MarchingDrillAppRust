//! Relay load on an 80-performer, 10-set show.
//!
//! Each editor drags one dot at frame rate and publishes presence the same
//! way. Drags and cursors are coalesced to 20Hz. The relay process is
//! separate so its CPU and RSS are not the clients. One client applies the
//! incoming batch on a 4× CPU throttle (the geometry walk repeated four
//! times), which is the mobile-like frame time.

use drill_core::{Document, Point, SetId};
use drill_presence::{Presence, PresenceClient, UserId};
use drill_sync::{DotId, ReplicaId, RoomEvent, ShowState, SyncClient, SyncOp};
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(2);
const COUNTS: &[usize] = &[2, 5, 10, 20, 30];

struct Row {
    editors: usize,
    p50_ms: f64,
    p95_ms: f64,
    cpu_pct: f64,
    rss_mb: f64,
    frame_p50_ms: f64,
    frame_p95_ms: f64,
    move_hz: f64,
    inbound_hz: f64,
    presence_hz: f64,
}

#[test]
fn relay_load_on_an_eighty_person_show() {
    let mut rows = Vec::new();
    for editors in COUNTS {
        rows.push(measure(*editors));
    }
    let mut table = String::from(
        "editors | e2e p50 ms | e2e p95 ms | relay cpu % | relay rss MB | frame p50 ms | frame p95 ms | move Hz | inbound Hz | presence Hz\n",
    );
    for row in &rows {
        table.push_str(&format!(
            "{:>7} | {:>10.1} | {:>10.1} | {:>11.1} | {:>12.1} | {:>12.2} | {:>12.2} | {:>7.1} | {:>10.1} | {:>12.1}\n",
            row.editors,
            row.p50_ms,
            row.p95_ms,
            row.cpu_pct,
            row.rss_mb,
            row.frame_p50_ms,
            row.frame_p95_ms,
            row.move_hz,
            row.inbound_hz,
            row.presence_hz,
        ));
    }
    eprintln!("\n{table}");
    std::fs::write("/tmp/drillforge-load.txt", &table).expect("write load table");
    for row in &rows {
        assert!(
            row.move_hz <= 26.0,
            "{} editors sent {:.1} drag Hz; throttle should hold near 20",
            row.editors,
            row.move_hz
        );
        assert!(
            row.move_hz >= 8.0,
            "{} editors only delivered {:.1} drag Hz",
            row.editors,
            row.move_hz
        );
        assert!(row.p95_ms.is_finite(), "missing latency samples");
        let presence_each = row.presence_hz / (row.editors as f64 - 1.0);
        assert!(
            (8.0..=26.0).contains(&presence_each),
            "{} editors delivered {presence_each:.1} presence Hz each",
            row.editors
        );
    }
}

fn measure(editors: usize) -> Row {
    let relay = Relay::spawn();
    let host = relay.address.clone();
    let document = big_show();
    let stop = Arc::new(AtomicBool::new(false));
    let ready = Arc::new(AtomicUsize::new(0));
    let sent = Arc::new(Mutex::new(HashMap::<u32, Instant>::new()));
    let mut handles = Vec::new();
    for index in 1..editors {
        let host = host.clone();
        let document = document.clone();
        let stop = Arc::clone(&stop);
        let ready = Arc::clone(&ready);
        let sent = Arc::clone(&sent);
        handles.push(std::thread::spawn(move || {
            drive_peer(index, editors, &host, &document, &stop, &ready, &sent);
        }));
    }

    let replica = ReplicaId::from_raw(1);
    let show = ShowState::from_document(&document, replica);
    let mut observer = SyncClient::connect(&host, "load", show.hello("Observer", [20, 96, 200]));
    let mut presence = PresenceClient::connect(
        &host,
        "load",
        Presence::new(UserId::from_raw(1), "Observer".into(), [20, 96, 200]),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let peers_ready = ready.load(Ordering::Relaxed) >= editors.saturating_sub(1);
        if peers_ready && observer.status() == drill_sync::ConnectionStatus::Connected {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        observer.status() == drill_sync::ConnectionStatus::Connected,
        "observer never connected"
    );
    assert!(
        ready.load(Ordering::Relaxed) >= editors.saturating_sub(1),
        "peers ready {} of {}",
        ready.load(Ordering::Relaxed),
        editors.saturating_sub(1)
    );
    std::thread::sleep(Duration::from_millis(200));

    let mut show = show;
    let mut document = document;
    let mut latencies = Vec::new();
    let mut frames = Vec::new();
    let mut moves_from_peer = 0u32;
    let mut sync_frames = 0u32;
    let mut presence_frames = 0u32;
    let cpu_start = cpu_ticks(relay.child.id());
    let rss_peak = Arc::new(AtomicU64::new(rss_kb(relay.child.id())));
    let sampling = Arc::new(AtomicBool::new(true));
    let sample_flag = Arc::clone(&sampling);
    let sample_peak = Arc::clone(&rss_peak);
    let pid = relay.child.id();
    let sampler = std::thread::spawn(move || {
        while sample_flag.load(Ordering::Relaxed) {
            let current = rss_kb(pid);
            sample_peak.fetch_max(current, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(50));
        }
    });

    let window_start = Instant::now();
    let mut seq = 0u32;
    let dot = DotId {
        set_id: 1,
        performer_id: 1,
    };
    while window_start.elapsed() < WINDOW {
        seq = seq.saturating_add(1);
        if let Some(op) = show.move_dot(
            dot,
            Point {
                x: seq as f32,
                y: 2.0,
            },
        ) {
            observer.publish(RoomEvent::Op { from: replica, op });
        }
        let mut cursor = Presence::new(UserId::from_raw(1), "Observer".into(), [20, 96, 200]);
        cursor.cursor = Some(Point {
            x: seq as f32,
            y: 2.0,
        });
        presence.publish(cursor);

        let mut batch = Vec::new();
        while let Some(event) = observer.poll() {
            sync_frames = sync_frames.saturating_add(1);
            if let RoomEvent::Op {
                op: SyncOp::MoveDot { dot, point, .. },
                ..
            } = &event
                && dot.performer_id == 2
            {
                moves_from_peer = moves_from_peer.saturating_add(1);
                let key = point.x.round() as u32;
                if let Some(sent_at) = sent.lock().expect("sent").get(&key).copied() {
                    latencies.push(sent_at.elapsed().as_secs_f64() * 1000.0);
                }
            }
            batch.push(event);
        }
        while presence.poll().is_some() {
            presence_frames = presence_frames.saturating_add(1);
        }
        if !batch.is_empty() {
            frames.push(throttled_frame_ms(&mut show, &batch, &mut document));
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    let elapsed = window_start.elapsed().as_secs_f64().max(0.001);
    let cpu_ticks = cpu_ticks(relay.child.id()).saturating_sub(cpu_start);
    sampling.store(false, Ordering::Relaxed);
    let _ = sampler.join();
    stop.store(true, Ordering::Relaxed);
    for handle in handles {
        let _ = handle.join();
    }
    drop(observer);
    drop(presence);

    Row {
        editors,
        p50_ms: percentile(&mut latencies, 0.50),
        p95_ms: percentile(&mut latencies, 0.95),
        cpu_pct: cpu_ticks as f64 / 100.0 / elapsed * 100.0,
        rss_mb: rss_peak.load(Ordering::Relaxed) as f64 / 1024.0,
        frame_p50_ms: percentile(&mut frames, 0.50),
        frame_p95_ms: percentile(&mut frames, 0.95),
        move_hz: f64::from(moves_from_peer) / elapsed,
        inbound_hz: f64::from(sync_frames) / elapsed,
        presence_hz: f64::from(presence_frames) / elapsed,
    }
}

fn drive_peer(
    index: usize,
    _editors: usize,
    host: &str,
    document: &Document,
    stop: &AtomicBool,
    ready: &AtomicUsize,
    sent: &Mutex<HashMap<u32, Instant>>,
) {
    let replica = ReplicaId::from_raw((index as u64) + 1);
    let mut show = ShowState::from_document(document, replica);
    let name = format!("Peer {index}");
    let mut client = SyncClient::connect(host, "load", show.hello(&name, [20, 96, 200]));
    let mut presence = PresenceClient::connect(
        host,
        "load",
        Presence::new(UserId::from_raw(replica.get()), name.clone(), [20, 96, 200]),
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline && client.status() != drill_sync::ConnectionStatus::Connected {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    ready.fetch_add(1, Ordering::Relaxed);
    let dot = DotId {
        set_id: 1,
        performer_id: (index as u32) + 1,
    };
    let mut seq = 0u32;
    while !stop.load(Ordering::Relaxed) {
        seq = seq.saturating_add(1);
        let point = Point {
            x: seq as f32,
            y: 3.0 + index as f32,
        };
        if index == 1 {
            sent.lock().expect("sent").insert(seq, Instant::now());
        }
        if let Some(op) = show.move_dot(dot, point) {
            client.publish(RoomEvent::Op { from: replica, op });
        }
        let mut cursor =
            Presence::new(UserId::from_raw(replica.get()), name.clone(), [20, 96, 200]);
        cursor.cursor = Some(point);
        presence.publish(cursor);
        let _ = client.poll();
        let _ = presence.poll();
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn throttled_frame_ms(show: &mut ShowState, batch: &[RoomEvent], document: &mut Document) -> f64 {
    let start = Instant::now();
    for event in batch {
        show.ingest(event);
    }
    show.write_document(document);
    let mut sink = 0.0f32;
    for _ in 0..4 {
        for set in &document.sets {
            for point in &set.positions {
                sink += point.x + point.y;
            }
        }
    }
    std::hint::black_box(sink);
    start.elapsed().as_secs_f64() * 1000.0
}

fn big_show() -> Document {
    let mut document = Document::demo(8, 10);
    let template = document.sets[0].clone();
    document.sets.clear();
    for index in 0..10 {
        let mut set = template.clone();
        set.id = SetId::new(index + 1).expect("set id");
        set.name = format!("Set {}", index + 1);
        for (step, point) in set.positions.iter_mut().enumerate() {
            point.x += index as f32 * 0.25;
            point.y += (step % 5) as f32 * 0.05;
        }
        document.sets.push(set);
    }
    assert_eq!(document.performers.len(), 80);
    assert_eq!(document.sets.len(), 10);
    document
}

fn percentile(samples: &mut [f64], fraction: f64) -> f64 {
    if samples.is_empty() {
        return f64::NAN;
    }
    samples.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((samples.len() - 1) as f64 * fraction).round() as usize;
    samples[index.min(samples.len() - 1)]
}

struct Relay {
    address: String,
    child: Child,
}

impl Relay {
    fn spawn() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("probe");
        let address = listener.local_addr().expect("addr").to_string();
        drop(listener);
        let mut child = Command::new(env!("CARGO_BIN_EXE_drill-relay"))
            .arg(&address)
            .env("DRILLFORGE_EDITOR_LIMIT", "64")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn relay");
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if std::net::TcpStream::connect(&address).is_ok() {
                return Self { address, child };
            }
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
        panic!("relay binary did not accept on {address}");
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn cpu_ticks(pid: u32) -> u64 {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return 0;
    };
    let Some((_, rest)) = stat.rsplit_once(')') else {
        return 0;
    };
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let utime: u64 = fields
        .get(11)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let stime: u64 = fields
        .get(12)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    utime + stime
}

fn rss_kb(pid: u32) -> u64 {
    let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
        return 0;
    };
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmRSS:") {
            return value
                .split_whitespace()
                .next()
                .and_then(|kb| kb.parse().ok())
                .unwrap_or(0);
        }
    }
    0
}
