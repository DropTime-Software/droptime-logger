//! modbus_emulator — a virtual MODBUS-TCP roaster on localhost.
//!
//! The no-hardware contributor rig for the generic MODBUS driver (the twin of
//! `tc4_emulator`; protocol doc: docs/protocols/modbus-generic.md): it starts
//! an in-process MODBUS-TCP server that answers FC03/FC04 reads from a roast
//! curve. Point a `modbus-tcp:127.0.0.1:<port>` session at it and you are
//! "connected to a roaster". Works on every OS — no pty needed.
//!
//! Usage:
//!
//! ```sh
//! cd apps/droptime-logger/src-tauri            # (apps/logger in the OSS repo)
//! cargo run --example modbus_emulator          # built-in synthetic roast
//! cargo run --example modbus_emulator -- \
//!     --fixture ../../../packages/roast-console/fixtures/ethiopia-guji.json \
//!     --speed 10 --port 1502                   # replay a fixture at 10×
//! ```
//!
//! * `--fixture <path>` — a roast-console `RoastFixture` JSON; the emulator
//!   serves BT/ET from its `curve` (t/bt/et, °F). Without it, a built-in
//!   synthetic curve (preheat → charge plunge → development → drop) plays.
//! * `--speed <N>` — time multiplier for the curve clock (default 1).
//! * `--port <N>` — TCP port to listen on (default 5020; 502 needs root).
//! * `--unit F|C` — unit the registers carry (default F).
//!
//! Default register map (any unit id accepted; keep in lockstep with the
//! in-crate test emulator and the protocol doc):
//!
//! | table   | address | value                                    |
//! |---------|---------|------------------------------------------|
//! | input   | 0       | BT × 10 (u16 — pin: dataType u16, scale 0.1) |
//! | input   | 1       | ET × 10 (u16)                            |
//! | input   | 2–3     | BT as f32, standard word order           |
//! | input   | 4–5     | ET as f32, standard word order           |
//! | input   | 6–7     | BT as f32, SWAPPED word order            |
//! | input   | 8       | heater duty % × 10 (constant 45.0)       |
//! | input   | 9       | fan duty % × 10 (constant 60.0)          |
//! | holding | 0       | BT × 10 (u16)                            |
//! | holding | 1       | ET × 10 (u16)                            |
//!
//! Reads outside the map answer exception `IllegalDataAddress`; any non-read
//! function code answers `IllegalFunction` plus a loud stderr warning — the
//! Logger must never send one (read-only invariant), and this rig makes any
//! violation impossible to miss.
//!
//! PROVENANCE (clean-room): MODBUS is an open, published standard (MODBUS
//! Application Protocol Specification V1.1b3, modbus.org). The register map
//! above is invented for this rig; it is NOT any vendor's map.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio_modbus::server::tcp::{accept_tcp_connection, Server};
use tokio_modbus::server::Service;
use tokio_modbus::{ExceptionCode, Request, Response, SlaveRequest};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut fixture_path: Option<String> = None;
    let mut speed = 1.0f64;
    let mut port: u16 = 5020;
    let mut fahrenheit = true;
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
            "--port" => {
                i += 1;
                port = args
                    .get(i)
                    .and_then(|s| s.parse::<u16>().ok())
                    .filter(|p| *p != 0)
                    .unwrap_or_else(|| usage_and_exit());
            }
            "--unit" => {
                i += 1;
                fahrenheit = match args.get(i).map(String::as_str) {
                    Some("F") | Some("f") => true,
                    Some("C") | Some("c") => false,
                    _ => usage_and_exit(),
                };
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

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("failed to build the tokio runtime");
    rt.block_on(serve(port, curve, speed, fahrenheit, label));
}

fn usage_and_exit() -> ! {
    eprintln!(
        "usage: cargo run --example modbus_emulator [-- --fixture <RoastFixture.json>] \
         [--speed <N>] [--port <N>] [--unit F|C]\n\n\
         Serves a MODBUS-TCP roaster on 127.0.0.1 (default port 5020).\n\
         Register map (documented in docs/protocols/modbus-generic.md):\n\
           input   0    BT x10 (u16; pin: dataType \"u16\", scale 0.1)\n\
           input   1    ET x10 (u16)\n\
           input   2-3  BT f32 (standard word order)\n\
           input   4-5  ET f32 (standard word order)\n\
           input   6-7  BT f32 (swapped word order)\n\
           input   8    heater duty % x10 (constant 45.0)\n\
           input   9    fan duty % x10 (constant 60.0)\n\
           holding 0    BT x10 (u16)\n\
           holding 1    ET x10 (u16)"
    );
    std::process::exit(2)
}

async fn serve(port: u16, curve: Curve, speed: f64, fahrenheit: bool, label: &str) {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("could not listen on {addr}: {e}"));
    let unit = if fahrenheit { "F" } else { "C" };

    println!("MODBUS-TCP emulator up — {label} at {speed}×");
    println!("listening on {addr} (any unit id; registers carry °{unit})");
    println!("source id for the Logger: modbus-tcp:127.0.0.1:{port}");
    println!(
        "example sourcePin: {{ \"sourceId\": \"modbus-tcp:127.0.0.1:{port}\", \"unitId\": 1, \
         \"channels\": [ {{ \"role\": \"bt\", \"register\": 0, \"kind\": \"input\", \
         \"dataType\": \"u16\", \"scale\": 0.1 }}, {{ \"role\": \"et\", \"register\": 1, \
         \"kind\": \"input\", \"dataType\": \"u16\", \"scale\": 0.1 }} ], \"unit\": \"{unit}\", \
         \"pollMs\": 1000 }}"
    );
    println!("(FC03/FC04 only; anything else is rejected loudly. Ctrl-C to quit)");

    let service = EmulatorService {
        state: Arc::new(EmuState {
            curve,
            speed,
            fahrenheit,
            anchor: Instant::now(),
        }),
    };
    let on_connected = move |stream, socket_addr| {
        let service = service.clone();
        async move { accept_tcp_connection(stream, socket_addr, move |_addr| Ok(Some(service.clone()))) }
    };
    let on_process_error = |err: std::io::Error| eprintln!("connection error: {err}");
    Server::new(listener)
        .serve(&on_connected, on_process_error)
        .await
        .expect("server failed");
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

struct EmuState {
    curve: Curve,
    speed: f64,
    fahrenheit: bool,
    anchor: Instant,
}

#[derive(Clone)]
struct EmulatorService {
    state: Arc<EmuState>,
}

impl Service for EmulatorService {
    type Request = SlaveRequest<'static>;
    type Response = Response;
    type Exception = ExceptionCode;
    type Future = std::future::Ready<Result<Response, ExceptionCode>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        std::future::ready(self.state.handle(req.request))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Table {
    Holding,
    Input,
}

impl EmuState {
    fn handle(&self, request: Request<'static>) -> Result<Response, ExceptionCode> {
        match request {
            Request::ReadInputRegisters(addr, cnt) => self
                .read_span(Table::Input, addr, cnt)
                .map(Response::ReadInputRegisters),
            Request::ReadHoldingRegisters(addr, cnt) => self
                .read_span(Table::Holding, addr, cnt)
                .map(Response::ReadHoldingRegisters),
            other => {
                eprintln!(
                    "!! REJECTED non-read-only request: FC{:02} — the Logger must never \
                     send anything but FC03/FC04 (read-only invariant)",
                    other.function_code().value()
                );
                Err(ExceptionCode::IllegalFunction)
            }
        }
    }

    fn read_span(&self, table: Table, addr: u16, cnt: u16) -> Result<Vec<u16>, ExceptionCode> {
        let t = self.anchor.elapsed().as_secs_f64() * self.speed;
        let (bt_f, et_f) = value_at(&self.curve, t);
        let et_f = et_f.unwrap_or(bt_f + 40.0);
        let conv = |f: f64| {
            if self.fahrenheit {
                f
            } else {
                (f - 32.0) * 5.0 / 9.0
            }
        };
        let (bt, et) = (conv(bt_f), conv(et_f));
        let end = u32::from(addr) + u32::from(cnt);
        (u32::from(addr)..end)
            .map(|a| self.register(table, a as u16, bt, et))
            .collect()
    }

    fn register(&self, table: Table, addr: u16, bt: f64, et: f64) -> Result<u16, ExceptionCode> {
        let x10 = |v: f64| (v * 10.0).round().clamp(0.0, 65535.0) as u16;
        let f32_hi = |v: f64| ((v as f32).to_bits() >> 16) as u16;
        let f32_lo = |v: f64| (v as f32).to_bits() as u16;
        match (table, addr) {
            (Table::Holding, 0) | (Table::Input, 0) => Ok(x10(bt)),
            (Table::Holding, 1) | (Table::Input, 1) => Ok(x10(et)),
            (Table::Input, 2) => Ok(f32_hi(bt)),
            (Table::Input, 3) => Ok(f32_lo(bt)),
            (Table::Input, 4) => Ok(f32_hi(et)),
            (Table::Input, 5) => Ok(f32_lo(et)),
            (Table::Input, 6) => Ok(f32_lo(bt)), // swapped: low word first
            (Table::Input, 7) => Ok(f32_hi(bt)),
            (Table::Input, 8) => Ok(450), // heater duty % ×10
            (Table::Input, 9) => Ok(600), // fan duty % ×10
            _ => Err(ExceptionCode::IllegalDataAddress),
        }
    }
}
