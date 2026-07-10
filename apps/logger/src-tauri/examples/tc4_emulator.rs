//! tc4_emulator — a virtual TC4/aArtisanQ roaster on a pty (macOS/Linux).
//!
//! The no-hardware contributor rig (see CONTRIBUTING.md): it opens a
//! pseudoterminal pair, prints the slave path, and speaks the TC4 serial
//! protocol on it. Point the app's setup wizard (or `sniff_port` /
//! `start_port_preview` / a `tc4:<path>` session) at the printed path and you
//! are "connected to a roaster".
//!
//! Usage:
//!
//! ```sh
//! cd apps/droptime-logger/src-tauri            # (apps/logger in the OSS repo)
//! cargo run --example tc4_emulator             # built-in synthetic roast
//! cargo run --example tc4_emulator -- \
//!     --fixture ../../../packages/roast-console/fixtures/ethiopia-guji.json \
//!     --speed 10                               # replay a fixture at 10×
//! ```
//!
//! * `--fixture <path>` — a roast-console `RoastFixture` JSON; the emulator
//!   answers `READ` from its `curve` (t/bt/et, °F). Without it, a built-in
//!   synthetic curve (preheat → charge plunge → development → drop) plays.
//! * `--speed <N>` — time multiplier for the curve clock (default 1).
//!
//! Protocol spoken (provenance: aArtisanQ `commands.txt` documentation,
//! BSD-3-Clause, greencardigan/TC4-shield — see
//! oss-staging/docs/protocols/tc4-aartisanq.md; clean-room, no GPL source):
//!
//! * `CHAN;xxxx` → `#`-ack;  `UNITS;F|C` → `#OK` (switches reporting unit);
//! * `FILT;…` → `#OK`;
//! * `READ` → `ambient,ch1,ch2,ch3,ch4,heater,fan` CSV (BT on logical 1,
//!   ET on logical 2);
//! * any CONTROL verb (OT1/OT2/IO3/PID/DCFAN/…) → `#REJECTED` plus a loud
//!   stderr warning — the Logger must never send one, and this rig makes any
//!   violation impossible to miss.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut fixture_path: Option<String> = None;
    let mut speed = 1.0f64;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--fixture" => {
                i += 1;
                fixture_path = Some(args.get(i).cloned().unwrap_or_else(|| usage_and_exit()));
            }
            "--speed" => {
                i += 1;
                speed = args
                    .get(i)
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|s| s.is_finite() && *s > 0.0)
                    .unwrap_or_else(|| usage_and_exit());
            }
            "--help" | "-h" => usage_and_exit(),
            other => {
                eprintln!("unknown argument: {other}");
                usage_and_exit()
            }
        }
        i += 1;
    }

    let curve = match &fixture_path {
        Some(path) => load_fixture_curve(path),
        None => builtin_curve(),
    };
    let label = fixture_path
        .as_deref()
        .unwrap_or("built-in synthetic roast");

    let pty = nix::pty::openpty(None, None).expect("openpty failed");
    // Raw mode so the pty moves bytes verbatim (no echo, no \n → \r\n).
    let mut termios = nix::sys::termios::tcgetattr(&pty.slave).expect("tcgetattr failed");
    nix::sys::termios::cfmakeraw(&mut termios);
    nix::sys::termios::tcsetattr(&pty.slave, nix::sys::termios::SetArg::TCSANOW, &termios)
        .expect("tcsetattr failed");
    let slave_path = nix::unistd::ttyname(&pty.slave).expect("ttyname failed");
    // Release our slave fd so clients can open the path exclusively; the pty
    // stays alive through the master we hold.
    drop(pty.slave);

    // Non-blocking master so the loop stays responsive.
    let flags = nix::fcntl::fcntl(&pty.master, nix::fcntl::FcntlArg::F_GETFL).expect("F_GETFL");
    let mut oflags = nix::fcntl::OFlag::from_bits_truncate(flags);
    oflags.insert(nix::fcntl::OFlag::O_NONBLOCK);
    nix::fcntl::fcntl(&pty.master, nix::fcntl::FcntlArg::F_SETFL(oflags)).expect("F_SETFL");
    let mut master = std::fs::File::from(pty.master);

    println!("TC4 emulator up — {label} at {speed}×");
    println!("virtual serial port: {}", slave_path.display());
    println!("(115200 8N1 semantics; Ctrl-C to quit)");

    let anchor = Instant::now();
    let mut fahrenheit = true;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        match master.read(&mut chunk) {
            Ok(0) => std::thread::sleep(Duration::from_millis(10)), // no client attached
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                    let raw: Vec<u8> = buf.drain(..=pos).collect();
                    let line = String::from_utf8_lossy(&raw).trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    let t = anchor.elapsed().as_secs_f64() * speed;
                    let reply = respond(&line, &curve, t, &mut fahrenheit);
                    write_all_nonblocking(&mut master, reply.as_bytes());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => {
                eprintln!("master read error: {e}");
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

fn usage_and_exit() -> ! {
    eprintln!(
        "usage: cargo run --example tc4_emulator [-- --fixture <RoastFixture.json>] [--speed <N>]"
    );
    std::process::exit(2)
}

/// (t seconds, bt °F, et °F) knots, linearly interpolated, held past the end.
type Curve = Vec<(f64, f64, Option<f64>)>;

fn builtin_curve() -> Curve {
    vec![
        (0.0, 390.0, Some(430.0)),   // preheat soak
        (60.0, 390.0, Some(430.0)),  // …one minute of it
        (140.0, 170.0, Some(300.0)), // charge plunge to the turning point
        (600.0, 410.0, Some(455.0)), // development ramp
        (660.0, 260.0, Some(300.0)), // drop plunge
        (3600.0, 200.0, Some(210.0)),
    ]
}

fn load_fixture_curve(path: &str) -> Curve {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("could not read fixture {path}: {e}"));
    let json: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("fixture {path} is not valid JSON: {e}"));
    let points = json["curve"]
        .as_array()
        .unwrap_or_else(|| panic!("fixture {path} has no curve array"));
    let curve: Curve = points
        .iter()
        .map(|p| {
            let t = p["t"].as_f64().expect("curve point t");
            let bt = p["bt"].as_f64().expect("curve point bt");
            let et = p["et"].as_f64();
            (t, bt, et)
        })
        .collect();
    assert!(!curve.is_empty(), "fixture {path} has an empty curve");
    curve
}

fn value_at(curve: &Curve, t: f64) -> (f64, Option<f64>) {
    match curve.iter().position(|&(kt, _, _)| kt >= t) {
        Some(0) => (curve[0].1, curve[0].2),
        Some(i) => {
            let (t0, bt0, et0) = curve[i - 1];
            let (t1, bt1, et1) = curve[i];
            if t1 <= t0 {
                return (bt1, et1);
            }
            let frac = (t - t0) / (t1 - t0);
            let bt = bt0 + (bt1 - bt0) * frac;
            let et = match (et0, et1) {
                (Some(a), Some(b)) => Some(a + (b - a) * frac),
                _ => None,
            };
            (bt, et)
        }
        None => curve
            .last()
            .map(|&(_, bt, et)| (bt, et))
            .unwrap_or((0.0, None)),
    }
}

fn respond(line: &str, curve: &Curve, t: f64, fahrenheit: &mut bool) -> String {
    let verb = line.split(';').next().unwrap_or("");
    match verb {
        "CHAN" => "# Active channels set\n".to_string(),
        "UNITS" => {
            *fahrenheit = !line.trim_end().ends_with('C');
            "#OK\n".to_string()
        }
        "FILT" => "#OK\n".to_string(),
        "READ" => {
            let (bt_f, et_f) = value_at(curve, t);
            let conv = |f: f64| {
                if *fahrenheit {
                    f
                } else {
                    (f - 32.0) * 5.0 / 9.0
                }
            };
            format!(
                "{:.2},{:.2},{:.2},0.00,0.00,45.00,60.00\n",
                conv(72.0),
                conv(bt_f),
                conv(et_f.unwrap_or(bt_f + 40.0)),
            )
        }
        other => {
            eprintln!(
                "!! REJECTED non-read-only command: {other:?} — the Logger must never \
                 send control verbs (read-only invariant)"
            );
            "#REJECTED\n".to_string()
        }
    }
}

fn write_all_nonblocking(master: &mut std::fs::File, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match master.write(bytes) {
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(e) => {
                eprintln!("master write error: {e}");
                return;
            }
        }
    }
}
