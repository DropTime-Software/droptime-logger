# Droptime Logger — Free Desktop Roast Logger: Technical Build Plan

**Prepared:** July 9, 2026. Supersedes the ship order in `droptime-live-roast-logging.md` (whose research facts remain load-bearing and are cited throughout). Extends `droptime-roaster-copilot.md` (the SaaS plan), whose Phase 6 "live probe capture, deliberately last" this document now promotes to its own product.

**Decisions this plan encodes (made July 9, 2026):**
1. **Standalone free wedge product** — not a console inside droptime-app. A genuinely best-in-class, beautiful, free roast logger that funnels into paid Droptime.
2. **Replacement, not companion** — no Artisan relay tier. Droptime Logger is the screen the roaster watches. Native capture only.
3. **Desktop app (Tauri 2.x)** — native Rust serial/HID/USB capture; the browser can never reach Phidgets HID or vendor-class USB, and a production logging tool needs desktop-grade reliability. The UI core is web tech and ships to the web too (marketing demo + companion viewer).
4. **Simulator-first** — no roasting hardware on hand; a replay simulator is the primary dev harness and doubles as the marketing-site live demo. One cheap real rig (Phidgets, ~$110) gets purchased for driver validation.
5. **Open source (added July 10)** — the logger ships publicly as OSS v0.1.0; the cloud/AI/platform layer stays commercial (Artisan→artisan.plus shape). Tactical release plan: **`droptime-logger-oss-v0.1.0.md`** (supersedes this doc's distribution assumptions where they conflict; the trademark guardrails, clean-room/provenance discipline, and driver matrix here remain binding).
6. **Positioning expansion** — Droptime repositions from "micro-roasteries" to **the all-in-one tool for coffee roasters of any size**. Competitive frame is head-on vs Cropster; the free logger is the wedge Cropster cannot match without cannibalizing itself. Commercial rigs (Phidgets, MODBUS/PLC machines) move up the hardware priority list accordingly.

**Research base:** 5-agent recon (July 9, 2026) over the droptime-app/droptime-web codebases, existing plan corpus, Tauri 2.x ecosystem, hardware-protocol/licensing landscape, and Clerk/Convex desktop-sync patterns. Confidence tags: **[high]** primary-source verified, **[med]** inferred/single-source — every [med] item appears in §16 (spikes).

---

## 1. Product definition

**Droptime Logger** (working name; see §17) is a free desktop app for macOS and Windows (Linux tier-2) that live-logs coffee roasts from real hardware with a best-in-class interface, works fully offline and anonymous forever, and syncs into the Droptime platform when signed in.

**The one-sentence pitch:** *Plug in your roaster. See the most beautiful roast screen ever made. Free, forever.*

**What "best in class" means, concretely:**
- Live BT/ET curve with RoR, phase-shaded background, and a **ghost overlay of the target profile** with live delta readout.
- Big glanceable telemetry: BT, ET, RoR, elapsed, current phase, live DTR, projected drop.
- One-tap event markers (CHARGE / DRY END / FC START / FC END / DROP) with huge touch/click targets and keyboard shortcuts — roasters have full hands.
- Phase progress bars: drying / Maillard / development with percentages, live.
- **Zero data loss**: samples hit local SQLite before they hit the screen; app crash or force-quit mid-roast → relaunch offers "Resume roast" with nothing missing.
- Post-DROP: instant summary screen; signed-in users get the AI readout (the existing Droptime pipeline, unchanged) within seconds.
- Dark, dim-roastery-friendly console on the Droptime design system (pine `#0D2418` canvas, forest `#024522` BT, coral `#fe6e51` RoR, lemon `#DAF698` accents, cream `#FDF7EE`, Inter −0.5px, Phosphor icons).

**Read-only invariant (unchanged from prior research):** capture only, no machine control in v1.x. The aArtisanQ protocol has control verbs (OT1/OT2/IO3/PID) — we never send them. Bullet's single-consumer constraint and Skywalker's 10s watchdog make control a liability the wedge doesn't need. [high]

**Honest scope of "replacement":** v1 is a *logging* replacement. A substantial slice of the TC4/Skywalker community mods their machines precisely to drive heater/fan/PID from Artisan — for them, a capture-only app cannot replace Artisan, and we've ruled out the companion/relay mode that would serve them. So: marketing scopes replacement claims to logging-only rigs (probe-equipped drums — which is also the Phidgets commercial audience), the closed beta (§14, Phase 2.5) quantifies the logging-only vs needs-control split before public copy commits, and machine control is an explicit v2 decision point, not a silent never.

### Conversion ladder

| State | What you get | What's gated |
|---|---|---|
| **Anonymous (default)** | Unlimited local roasts, full console, simulator, .alog import/export, local history & comparison | No cloud, no AI, no sharing |
| **Signed in — Hobbyist (free)** | Cloud sync, roast history everywhere, **20 AI actions/mo** (readouts + follow-up Q&A share one counter — that is what PLAN_LIMITS meters; don't market it as "20 readouts"), live companion viewer, 1 machine (limit newly enforced — §10/§11) | Inventory, blends, planning, team; share links land v2.x |
| **Roastery $79 / Pro $149** | Full platform (existing tiers, unchanged) | — |

No new billing machinery: sign-in lands users in the existing hobbyist tier (`billingStatus:"free"`, no Stripe object). One change required: the hobbyist AI cap must become a **hard** gate — and it must live in `recordAiAction` itself (or a shared `requireAiBudget` pre-check used by **every** Claude entry point: `generateReadoutAI`, `askRoastQuestion`, chat, cupping, blend transitions, `regenerateReadout`). Today the meter is soft everywhere — flags overage, never blocks — so gating only the readout path would leave a signed-in free user unlimited Claude via Q&A/chat: the exact cost leak the gate exists to close. In-flight behavior: complete the current turn, gate the next; the readout path writes a gated row the UI renders as an upgrade card, other modules return a typed gate error. This explicitly supersedes the copilot plan's acceptance note that the hobbyist's 21st action is "allowed as flagged overage (not blocked)" — that stance predates funneling a free desktop product into the API bill. Paid tiers stay soft per the "never a hard cutoff mid-workflow" pledge. [high]

**PLG mechanics that carry over from the suite:** the persistent, never-error-styled, benefit-led upsell surface (the demo-countdown-banner philosophy) becomes the logger's "Sign in to sync + get AI readouts" band. The demo-org 14-day expiry machinery does **not** apply — the logger is free-forever with real local data; anonymous is a first-class state, not an onboarding-gated throw. [high]

### Competitive frame (verified July 2026)

- **Artisan**: free, GPL, powerful, looks like a lab instrument; the UX we replace. Fast release cadence.
- **Cropster**: the all-in-one incumbent for commercial roasteries. Pricing now calculator-gated: Starter = 10 free roasts/mo, +€26/mo per 200 roasts, volume charges after 250 kg/mo, add-ons run €100–500/mo. **Acquired Firescope (Oct 2025)** — a Seoul-founded all-in-one small-roaster app (~3,000 users across Korea/Japan/Asia, 0.5s sampling, auto event detection), kept as a standalone brand: Cropster is already executing the down-market-without-cannibalizing play via acquisition. Don't quote their mid-tier prices in marketing (calculator-gated, unverifiable). [high]
- **RoasTime 4**: free, Electron, polished — but Bullet-only. The UX benchmark for one machine brand. [high]
- **RoastLog**: alive (© 2025), flat-pricing, proprietary bridge hardware + Phidgets. [high]
- **Rostoc** (rostoc.co): **free-to-start desktop app for macOS/Windows** (adversarial review corrected earlier recon — it is not web-based and not capture-blocked). Lists connection paths for Phidgets, MODBUS TCP/RTU, and USB serial, with setup guides for Kaleido, Giesen, Probat P, Siemens S7, and Bullet workflows — though much is labeled experimental/planned and Kaleido logging is read-only. **The closest direct competitor; assume they execute.** [high — re-verified against rostoc.co July 2026]
- **HiBean** (hibean.fun): mobile-first (iOS/iPad/Android) multi-brand live logger — Kaleido, Skywalker, Smola, Hamid, plus their own "Arc" logger hardware — with cloud sync. Contests the low end from mobile; no desktop console, no AI readout advertised. [med]
- **The honest wedge**: native capture alone is NOT a moat — Rostoc ships it too. What nobody combines: free-forever **anonymous local-first** capture + a zero-data-loss desktop architecture + an AI readout grounded in the roaster's own history + a full platform behind it. That compound — plus console quality — is the differentiation; marketing must not lean on "competitors can't capture natively." The desktop bet vs the mobile contenders (HiBean, Firescope) is deliberate and should be stated: a production roast is run from a fixed screen beside the machine, and probe/PLC hardware hangs off USB/LAN, not a phone. [high]

---

## 2. Ship shape & repo layout

Two new workspaces plus surgical changes to two existing apps:

```
apps/droptime-logger/            # NEW — the desktop app
  package.json                   # @lemon/droptime-logger; scripts: dev/build/lint/clean (web parts only)
  vite.config.ts                 # Vite + React SPA (not Next — desktop webview wants a static SPA)
  src/                           # console app shell, auth bridge, sync UI, settings
  src-tauri/                     # Rust core
    tauri.conf.json              # devUrl :5177, frontendDist ../dist, updater/deep-link config
    src/
      main.rs                    # plugin registration (single-instance FIRST, then deep-link, opener, updater…)
      capture/                   # engine + DeviceSource trait + drivers (one module per driver + PROVENANCE.md)
      store/                     # rusqlite layer: schema, WAL, roast writer, outbox
      sync/                      # outbox flusher scaffolding (v1 flush logic lives webview-side; see §10)
      ipc.rs                     # Tauri commands + Channel<Sample> streaming to webview
packages/roast-console/          # NEW — @lemon/roast-console: shared UI core
  # LiveRoastChart (live-mode fork of RoastCurveChart), BigNumbers, PhaseBars,
  # EventMarkerBar, RoastSummary, simulator engine (TS), curve math (trailing RoR,
  # phase calc, projections), design tokens. Consumed by: droptime-logger (Vite),
  # droptime-web (demo page), droptime-app (companion viewer, later).
apps/droptime-app/               # EXISTING — backend additions only (§11)
apps/droptime-web/               # EXISTING — repositioning + /download + /demo (§12)
```

**Monorepo mechanics** [high]: `pnpm-workspace.yaml` already globs `apps/*`/`packages/*` — new dirs auto-join. Match the standard script names (`dev`, `build`, `lint`, `clean`) so turbo picks them up; the logger's turbo `build` covers **only the Vite frontend** (outputs `dist/**`) — `tauri build` (platform bundles, signing) stays out of the turbo graph entirely and runs in CI via tauri-action (`projectPath: apps/droptime-logger`). Exclude `src-tauri/target` from git and turbo inputs. Vercel deploy guards are unaffected: all twelve git-connected Vercel projects (ten apps/webs + two legacy golf sites) carry `turbo-ignore` and correctly skip for logger-only changes; the logger is not a Vercel project.

**Toolchain versions (verified current, July 2026)** [high]: tauri 2.11.x (stable line since Oct 2024, no v3 planned), @tauri-apps/api 2.11.x, serialport 4.9.0, nusb 0.2.4, hidapi 2.6.6, phidget 0.4.1 (+ libphidget22 ≥ v1.11.20220822, the BSD relicense point — **built from BSD source in our CI and bundled by us**: phidget-sys only link-flags a system-installed library, and Phidgets' prebuilt MSI/macOS Framework are governed by their restrictive EULA, not BSD — never redistribute those artifacts; point phidget-sys at our build via PHIDGET_ROOT), rusqlite 0.40.x `bundled`, tauri-plugin-deep-link 2.4.x, -single-instance 2.4.x (register FIRST, `deep-link` feature on), -opener 2.5.x, -updater 2.10.x, keyring 4.1.x (Stronghold is deprecated — do not use), tauri-plugin-sentry 0.5.0 (pin exact), py_literal (MIT/Apache) for .alog.

---

## 3. Architecture overview

```
┌────────────────────────── Tauri app ──────────────────────────┐
│  Rust core (never throttled)          Webview (React/Vite)    │
│  ┌──────────────┐                     ┌────────────────────┐  │
│  │ Capture engine│──Channel<Sample>──▶│ Live console UI    │  │
│  │ thread/device │                    │ (@lemon/roast-     │  │
│  └──────┬───────┘                     │  console)          │  │
│         │ write-ahead (same tick)     │                    │  │
│  ┌──────▼───────┐   invoke (read/ack) │ Convex JS client   │  │
│  │ SQLite (WAL) │◀──────────────────▶│ + ClerkJS (native  │  │
│  │ roasts/samples│                    │   mode, §9)        │──┼──▶ Convex cloud
│  │ /events/outbox│                    └────────────────────┘  │    (droptime-app
│  └──────────────┘                                             │     deployment)
└───────────────────────────────────────────────────────────────┘
```

**The two hard rules the recon burned in** [high]:
1. **Capture + persistence live in Rust.** Webview timers throttle when minimized on Windows/Linux and Tauri's `backgroundThrottling: "disabled"` is WKWebView-only. A minimized window must never gap the roast log. The webview only renders; on un-minimize it catches up from the Channel/DB.
2. **Samples reach SQLite before the UI.** The capture thread's tick is: read hardware → append to `samples` (batched per-second transaction) → emit on `tauri::ipc::Channel<Sample>` (ordered; events are for lifecycle only, async event listeners can reorder). A crash after the write loses nothing.

**Rendering note** [high]: charts stay SVG/Canvas2D — never WebGL. Linux webkit2gtk silently software-renders WebGL and masks the renderer string; at ≤ a few thousand points, SVG paths at 1–2 Hz are trivially cheap (proven by the existing RoastCurveChart architecture). Ship the documented Linux env-var workarounds (`WEBKIT_DISABLE_DMABUF_RENDERER=1` etc.) settable in `main()`, and treat Linux as tier-2 at launch.

---

## 4. Capture engine & driver matrix

### DeviceSource abstraction

```rust
trait DeviceSource: Send {
    fn descriptor(&self) -> DeviceDescriptor;          // id, kind, display name, channels offered
    fn start(&mut self, tx: SampleSink) -> Result<()>; // spawns/owns its read loop
    fn stop(&mut self);
}
struct Sample { mono_ms: u64, wall_ms: u64, channels: SmallVec<ChannelReading> } // °F canonical
enum ChannelKind { BT, ET, Ambient, Aux(u8), Heater, Fan, Drum }                 // maps to curve{bt,et,burner,airflow,drum}
```

Sampling default 1s tick (configurable 1–5s; industry norm, Cropster defaults 1s, Artisan 2s). Raw samples persist verbatim; **all smoothing/RoR is display-side and server-side** — Artisan's proven model, already Droptime's. [high]

**Discovery/hotplug**: `serialport::available_ports()` exposes USB VID/PID/serial metadata on all three OSes → auto-detect known hardware and preselect the right driver. serialport-rs has no hotplug events; use `nusb::watch_devices()` (real OS hotplug on all 3 platforms) as the re-enumeration trigger, 2s polling as fallback. Roll our own thin serial layer on serialport 4.9.0 — do **not** adopt tauri-plugin-serialplugin (v3.0.0 shipped 2026-07-08 with breaking changes; Windows enumeration via deprecated `wmic`), and raw-serial-to-JS is the wrong layer anyway. [high]

### Driver matrix, in build order (revised for the any-size positioning)

| # | Driver | Transport | Licensing path | Hardware for testing | Status |
|---|---|---|---|---|---|
| 0 | **Simulator/replay** | in-process | ours | none | dev harness + demo (§8) |
| 1 | **Phidgets** (VINT HUB0000/HUB0001 + TMP1101 4×TC; legacy 1048 direct-USB) | libusb userspace via libphidget22 (no kernel drivers, any OS) | **fully clean with one discipline**: libphidget22 is BSD-3-Clause since v1.11.20220822 (older copies are LGPL); `phidget` Rust crate is MIT; we build the C lib from BSD source in CI and bundle our artifact (§2) — Phidgets' own installers are EULA-bound | **BUY: ~$75–110** (hub + TMP1101 + 2× K-probes) — the exact rig commercial drums run; Cropster documents the identical setup | v1 launch |
| 2 | **TC4/aArtisanQ serial** | USB serial 115200 8N1, ASCII `READ` → `ambient,ch1,ch2,...` CSV; `CHAN`/`UNITS`/`FILT` config; `#`-prefixed non-data lines | **fully clean**: protocol spec (commands.txt + firmware headers) is BSD-3-Clause (MLG Properties/Jim Gallt) — cite it freely | TC4+ was **retired 2025**; test via ESP32 TC4-emulators (e.g. FilePhil/TC4-Emulator pattern), clones, community testers | v1 launch — covers the whole TC4-emulating DIY/Skywalker-mod ecosystem |
| 3 | **Generic MODBUS-TCP poller** | LAN to machine PLC | **fully clean**: open standard, tokio-modbus (MIT/Apache) | none needed for the generic layer; per-machine register maps from vendors/community (Loring: fixed IPs 192.168.1.199/.69, BCD decode; Probat Wago/Beckhoff; Giesen W-series) | **v1.0 ship gate** — user-configurable (host, unit, registers, decode incl. BCD), mirroring Artisan's generic MODBUS device; this is the Loring/Giesen/Probat door for larger roasteries, and the "any size" positioning (§12) is not credible without it |
| 4 | **Kaleido** (M-series/Sniper) | legacy serial 9600/57600 (CP210x); 2023+ "Board-C" Serial/Network protocol | **gated on vendor docs**: only public implementations are GPL; Kaleido is a paying Artisan machine sponsor with proven integrator cooperation — **email them in week 1** | vendor docs + community testers; own-device sniffing as fallback | v1.x, vendor-gated |
| 5 | **Aillio Bullet R2** | **USB CDC-ACM serial — confirmed by Aillio's own docs** ("Bullet R2 CDC" in Device Manager; VID 0x0483, PIDs 0x5741/0xa27e in their udev rules; PID→model mapping unverified) | **the one true clean-room project**: byte-level protocol exists publicly only in Artisan's GPL aillio.py — no MIT/Apache implementation in any language. Path: USB-capture RoasTime↔Bullet on owned/borrowed hardware (own-traffic facts are clean), or two-person spec/implementer separation | needs a unit or committed community tester; must detect RoasTime running (single-consumer — concurrent clients crash) | v2 |
| 6 | Bullet R1 | vendor-class USB via nusb (WinUSB on Windows — RoasTime installs it; document Zadig fallback) | same clean-room project as R2 | same | v2 |

**Per-driver provenance discipline** [high]: every protocol-touching module — drivers **and the .alog parser/writer** — carries a `PROVENANCE.md` recording exactly which sources informed it (BSD firmware files, vendor docs, own USB captures, GitHub issue threads). Implementing a protocol from *facts* is not a GPL event (17 U.S.C. §102(b); the Sega v. Accolade / Sony v. Connectix interop line; Directive 2009/24/EC Art. 1(2); SAS v. WPL C-406/10 — cite Google v. Oracle only as fair-use belt-and-suspenders; it assumed copyrightability arguendo). Transcribing *expression* is — so the discipline targets process:
- **Prefer the capture-only path.** Protocol facts observed from our own device traffic need no clean room. EU/UK captures additionally sit under the non-waivable observation right (Directive 2009/24/EC Art. 5(3)).
- **If GPL source must be consulted** (Bullet worst case): a real clean room — spec writer produces a facts-only protocol spec (no code excerpts, verbatim comments, constant tables as-expressed, or identifier names); a second reviewer scrubs it for expressive content; the spec is retained permanently; the implementer signs a non-access attestation and the spec writer never touches driver code. This applies to agents too: **the implementing session's context must exclude Artisan source**, and PROVENANCE.md logs the session's inputs.
- **Phase 0 item:** install RoasTime and read its actual click-through EULA; record any anti-reverse-engineering clause in the Bullet PROVENANCE.md (contract claims like Bowers v. Baystate are the real exposure, not copyright) and prefer EU/UK-jurisdiction captures or genuinely voluntary community testers on their own equipment accordingly.

**Platform driver notes** [high]: Phidgets = zero driver install on Windows (inbox HID)/macOS; Linux needs udev rules (ship in .deb, document for AppImage). Bullet on Windows rides RoasTime's WinUSB install. macOS vendor-class USB works userspace by default; Phidgets' vendor-defined HID usage pages shouldn't trip the Input Monitoring TCC prompt, but test explicitly (known failure mode: silent IOHIDDeviceOpen denial).

---

## 5. Local data layer (SQLite)

rusqlite (`bundled`), one writer thread, in `app_local_data_dir()` (NOT roaming `%APPDATA%` on Windows — roast DBs shouldn't ride roaming profiles). [high]

```sql
PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; -- corruption-proof; worst case loses the last unsynced ~1s batch
-- busy_timeout set; wal_checkpoint(TRUNCATE) after each roast ends

roasts        (uuid TEXT PK,            -- UUIDv7, minted at CHARGE; permanent identity, survives sign-in
               device_id TEXT,          -- per-install UUID
               machine_local_id, coffee_name, charge_weight_lb, started_wall_ms,
               status TEXT,             -- recording | finished | abandoned
               charge_temp_f, turning_point_sec, turning_point_temp_f, dry_end_sec,
               fc_start_sec, fc_end_sec, drop_sec, drop_temp_f, drop_weight_lb,
               notes, synced_batch_id TEXT NULL)
samples       (roast_uuid, seq INTEGER, t_sec REAL, bt_f REAL, et_f REAL NULL,
               ambient_f REAL NULL, heater REAL NULL, fan REAL NULL, drum REAL NULL,
               PRIMARY KEY (roast_uuid, seq))          -- append-only, full resolution, never rewritten
events        (roast_uuid, kind TEXT, t_sec REAL, created_ms)   -- marker taps, corrections kept as history
outbox        (id INTEGER PK, roast_uuid, op TEXT,     -- start_live | chunk | live_patch | finalize | import
               chunk_index INTEGER NULL, payload BLOB, created_ms, synced_ms NULL)
settings, machines_local, csv_presets                   -- port→driver pins, unit display pref, etc.
```

- Batch inserts in per-second transactions (never autocommit-per-row).
- **Crash recovery**: on launch, any `roasts.status='recording'` → "Resume roast?" — capture engine reattaches to the device and continues seq numbering; if the device is gone, the roast is closed at last sample with a gap marker.
- Markers are editable post-roast (tap timing is human); `events` keeps the raw taps, `roasts` holds the canonical values.
- Retention: local data is never auto-deleted. Export: `.alog` (via our own writer), CSV, JSON.
- Canonical units everywhere: °F, seconds-from-charge — matching the platform. °C/°F display toggle (chart, big numbers, deltas, exports) is a **Phase 1 deliverable**, not polish — the head-on-Cropster world is metric.
- **Schema migrations from day one**: `PRAGMA user_version` ladder run at startup; the DB file is backup-copied before any migration; outbox payloads carry a version field and the flusher must handle all prior versions. This app auto-updates on top of a never-deleted local history — an unversioned schema is a data-loss bug scheduled for release two. Phase 3's updater exit criteria include "update mid-history: old roasts intact, outbox drains."

---

## 6. Console UI (`@lemon/roast-console`)

**Chart**: a live-mode fork of the proven bespoke inline-SVG RoastCurveChart (506 lines, no chart lib — recharts/@tremor in droptime-app's package.json are confirmed dead deps; don't resurrect them). The SVG-path architecture handles 1–2 Hz appends trivially at ≤ 700 points. What the fork adds, per recon [high]:
- **Stable axes**: hysteresis domains (grow-only during a roast, padded), never per-sample rescaling jitter.
- **Leading-edge follow**: x-domain = max(profile target span, elapsed + 2 min), so the curve never hits the right wall.
- **Incremental trailing-window RoR** at the leading edge (the server's 30s symmetric window would lag ~15s live); on DROP, the stored roast is re-derived server-side with the symmetric window on sync — consistent with every imported roast. Reuse `deriveRoR`'s dropout rules (skip bt≤0, >120°F/min inter-sample spikes, flat-lined-probe → undefined not false-stall).
- Ghost target overlay (`roastProfiles.targetCurve` shape `{t,bt,ror?}`) + live delta chip ("+7°F vs target"); phase shading keyed off live markers; touch targets alongside mouse.
- Dark console theme (the existing chart hardcodes light-theme hex — tokenize).

**Console layout**: chart center-stage; top band of big numbers (BT, ET, RoR, elapsed, phase, live DTR once FC lands, projected drop from RoR extrapolation); bottom band = the marker bar (CHARGE auto-arms on first stable temp rise, then DRY / FC / FC END / DROP as huge buttons with spacebar/keyboard bindings); left rail = roast setup (machine, coffee free-text or synced catalog, charge weight, target profile picker). Post-DROP → summary screen (final curve, markers, DTR, weight-loss entry) → "AI readout" (signed-in) or "Sign in to get your readout" (the conversion moment).

**Sounds/cues**: optional audio ticks at projected FC and target drop approach. Small, but roasters love it and Artisan buries it.

**Device setup & failure UX — a named v1 workstream, not polish.** The setup wizard is itself a competitive weapon: incumbents treat hardware setup as an enterprise install, and the switching pitch is "plug in, hit Scan, roasting in three minutes." The **hardware sniffer**: on Scan, enumerate serial ports (VID/PID metadata first), and for unrecognized ports briefly listen and classify the incoming frame shape against known protocol signatures (TC4 CSV lines, MODBUS frames, Kaleido patterns) — "Found a TC4-compatible device on port 3, applying configuration" beats a baud-rate dropdown. Alongside it, the **no-proprietary-hardware stance** is explicit marketing: a "Don't have a digital roaster?" path recommending a standard ~$35 Phidgets TMP1101 + K-type probes (or any TC4 rig) with a visual probe-placement guide — against incumbents' proprietary connector boxes. First-run flow: detect device → **assign channel roles** (the flagship TMP1101 has four TC inputs; which is BT and which is ET is a user decision made against a live per-channel preview) → probe-type/protocol config where needed (TC4 `CHAN`/`UNITS`) → optional calibration offset → saved as the machine's pin in `machines_local`; manual port picker when VID/PID auto-detect fails. Live failure matrix with specified console behavior: probe unplugged mid-roast (banner + gap marker, capture continues on remaining channels), device replug/resume mid-roast, garbage frames, flat-lined probe (reuse `deriveRoR`'s detection as a live banner), and — the single most predictable support case for migrating Artisan users — **the port already held by a running Artisan instance**: detect it and say so in plain words. Phase exit criteria exercise this matrix, not just the happy path.

---

## 7. What stays consistent with the platform (binding contract)

From the codebase recon — the logger must not invent parallel conventions [high]:
1. °F canonical, seconds-from-charge canonical.
2. Curve point shape `{t, bt, et?, ror?, burner?, airflow?, drum?}` — control-channel slots already exist.
3. Marker vocabulary: `chargeTempF, turningPointSec/TempF, dryEndSec, fcStartSec, fcEndSec, dropSec, dropTempF` (+ derived `dtr`); `needsReview` = missing FC.
4. Server recomputes RoR (`deriveRoR`) and decimates to ≤ 301 points (`decimate(curve, 300)`) on insert — the logger sends **raw full-resolution** samples and keeps its own full copy; the full-res session also uploads as the raw file (§10), finally implementing the schema's dormant `rawFileId`.
5. `source: "probe"` — reserved in the enum since day one, used nowhere yet; the logger claims it.
6. Design system + Phosphor icons; analytics events via the established pattern (new: `roast_synced_from_probe`, `logger_installed`, `logger_signed_in`).
7. The auto-scheduled AI readout on insert (`insertBatch` → `scheduler.runAfter(0, internal.ai.generateReadoutAI)`) — a synced live roast gets its readout with **zero new AI code**.

---

## 8. Simulator & fixtures

**TS simulator** (in `@lemon/roast-console`, runs in any browser): replay engine that streams curve points at 1×–20× with optional probe-noise/dropout injection, implementing the same TS `SampleSource` interface the Tauri Channel bridge implements — the console cannot tell simulation from hardware. Seeds: the three existing synthetic generators (seed.ts `makeRawCurve` with its stall anomaly, lib/artisan.ts `synthesizeCurve` + `sampleArtisanRoast`, droptime-web `buildRoastCurve` with numerically-integrated RoR). [high]

**Rust replay driver** (driver #0): parses .alog via `py_literal` and replays through the full native path — capture engine → SQLite → outbox → UI — so the entire pipeline is exercised end-to-end with zero hardware.

**Fixture gap + fix** [high]: zero real .alog files exist in the repo; all curves are synthetic and won't exercise real probe noise/timing. Sources for real fixtures: (a) **run Artisan's built-in simulator mode ourselves** to produce genuine .alog files structurally identical to real captures (files we generate are ours); (b) community-shared .alog files with permission; (c) first Phidgets rig sessions (probe in a toaster oven/heat gun produces real noise profiles). The .alog **format** is legally clean to parse/write (formats aren't copyrightable; Cropster/RoastLog/Rostoc all import it commercially) — but our parser must be written from sample files + the documented key schema (`timex`, `temp1`=ET, `temp2`=BT, `timeindex[8]` = CHARGE/DRYe/FCs/FCe/SCs/SCe/DROP/COOL, `specialevents*`, mode C/F), never from Artisan source. Permitted schema sources, explicitly: self-generated Artisan-simulator files with semantics established empirically, Artisan GitHub issue threads (#219, #1737), and third-party writeups — the parser/writer module carries a `PROVENANCE.md` like any driver (§4). Port note: the existing TS parser's quote-replace JSONification corrupts files containing apostrophes — `py_literal` in Rust is the correct approach, and the TS parser should eventually adopt a real Python-literal parse too.

**The demo IS the simulator**: droptime-web gets a pure-client `/demo` page — the same console components replaying a real roast at 10× with a "this is a replay" chip. No backend (the site is static by spec). This is the landing page's hero asset.

---

## 9. Auth: Clerk without official desktop support

Clerk has **no desktop/Tauri support and closed the request as not-planned** (clerk/javascript#4725); webview cookies + Clerk's Origin/Authorization rejection break naive embedding. The pattern below is the best-evidenced path but is **community-uncharted end-to-end [med]** — the individual pieces are proven, the specific combination is not, which is exactly why it's the Phase-0 spike:

1. **System browser + single-use sign-in token handoff**: "Sign in" opens `app.trydroptime.com` sign-in in the system browser (tauri-plugin-opener). After session establishment, a new endpoint calls Backend API `signInTokens.createSignInToken({userId, expiresInSeconds: 60})` and redirects to `droptime-logger://auth?token=...&state=<app-nonce>`.
2. Deep link arrives (tauri-plugin-deep-link; single-instance plugin registered FIRST forwards it on Windows/Linux where the URL spawns a second process). App verifies the nonce, then consumes the ticket via ClerkJS **native mode** (Expo-style, Authorization-header/`__client` token, no cookies): `signIn.create({strategy:'ticket', ticket})` — pin this exact call; the newer `signIn.ticket()` wrapper has broken on this flow (clerk/javascript#8219).
3. ClerkJS in the webview then owns the ~60s JWT refresh cycle against the `convex` JWT template; Clerk FAPI calls route through Rust HTTP (tauri-plugin-http) to dodge the Origin header problem. Caveat on provenance: the community `tauri-plugin-clerk` proves the fetch-proxy technique, but it does so with **standard** ClerkJS + cookie persistence — not native mode. Our native-mode + keyring combination is unproven, so the spike carries an explicit fork: (a) native mode with keyring-persisted `__client`; (b) fallback to the plugin's proven standard-mode + fetch-proxy + persisted-cookie technique (reference implementation, not dependency).
4. The long-lived `__client` token persists in the OS keychain via **keyring 4.x** (macOS Keychain / Windows Credential Manager / Linux dbus-secret-service — the keyutils backend doesn't survive reboots; fall back to an encrypted file with clear messaging when no secret service exists). macOS keychain access is tied to code-signing identity — stable once we sign consistently; unsigned dev builds re-prompt.
5. **Session lifetime**: Clerk max-lifetime defaults to 7 days; customizing production requires a paid Clerk plan, and sessions can never be permanent. Raise to 30–90 days, and treat expiry as a normal state: **signed-out never blocks logging** — the app degrades to local-only and re-auths via the browser flow. This keeps the free-forever-local promise structurally true.
6. Convex connection: standard JS client in the webview, `setAuth(fetchToken)` from ClerkJS; the Convex WebSocket protocol has no origin allowlist — desktop just works. (convex-rs 0.10.x exists and is actively maintained if sync ever moves into Rust; not needed for v1.)

**Platform gotchas to plan around** [high]: macOS deep links only work for a bundled app installed in /Applications (no runtime registration) — the OAuth callback is untestable under plain `tauri dev`; use loopback-localhost redirect (tauri-plugin-oauth) as the dev-mode and Linux-AppImage fallback (AppImage scheme registration breaks if the file moves). Onboarding order matters: new sign-ins must route through org creation + `chooseFreePlan` before first sync — `billingStatus:"unpaid"` throws `PAYWALL_REQUIRED` on every roast mutation, which would look like "sync is broken."

---

## 10. Sync protocol (durable outbox → Convex)

**Convex reality check** [high]: no production offline-first client exists (Curvilinear is a buggy alpha); the built-in reconnect queue is in-memory per session — it does not survive restart. The durable SQLite outbox is therefore mandatory, and it's the blessed community pattern. All limits are comfortable: a 12-min roast at 1 Hz ≈ 720 samples ≈ tens of KB against a 1 MiB doc cap; flushes every 2–5s are a few hundred bytes; ~240 mutations/roast ⇒ thousands of live-streamed roasts/month inside even the free Convex tier.

**The doc-shape rule that must be in the schema from day one** [high]: never grow a sample array on a subscribed doc — reactive queries re-read the whole doc per patch, so cost = writes × subscribers × read-set (the OpenClaw 9TB/day lesson; O(n²) over a roast). Instead:

```
liveRoasts        { organizationId, clientRoastId, machineId, status: recording|finished|stale,
                    btF, etF?, rorFPerMin?, phase, elapsedSec, lastSampleAt, startedAt }
                    -- ~400 bytes, PATCHED every 2-5s, change-detected (skip write if values unchanged)
roastSampleChunks { organizationId, clientRoastId, chunkIndex, samples: [{t,bt,et?,...}] }
                    -- fixed 30-60s windows, append-only, index by_org_client_chunk
```

Viewer subscribes to the tiny live doc + `order('desc').take(2)` on chunks (history fetched once, non-reactively). Client-side: **single-flight the live-doc patch** (absolute state — dropping intermediates is safe); **never single-flight chunk appends** (deltas — they flow only from the durable outbox in order).

**Sync ops** (all idempotent, all keyed on client identity):
- `logger.upsertMachine {clientMachineId, name, make?, batchCapacityLb?}` — idempotent by `(org, clientMachineId)`; creates/maps the **required** `machines` doc (`insertBatch` demands a real `Id<"machines">`, and `machines.batchCapacityLb` is a required field — default it to the largest charge weight seen, editable later). Runs before first sync. The hobbyist **1-machine limit is enforced here** (typed error the logger UI explains); bulk import over the limit maps extra local machines onto the allowed one with a tag rather than hard-failing.
- **Coffee resolution**: the client sends either a synced `coffeeId` or free text; free text **upserts a `coffees` doc by (org, normalized name)** — name-dedupe is the accepted tradeoff. This is not optional: `roastBatches` has no coffee-name field, and without a `coffeeId` the readout's prior-batch comparisons, the ghost overlay, and "Analyze this roast of ${coffeeName}" all silently degrade.
- `logger.startLiveRoast {clientRoastId, deviceId, machineId, coffeeId?, …}` — upsert by clientRoastId.
- `logger.appendSampleChunk {clientRoastId, chunkIndex, samples}` — insert-if-absent on `(clientRoastId, chunkIndex)`; Convex mutations are serializable OCC transactions, so check-then-insert is race-safe with no extra machinery.
- `logger.patchLive {…}` — change-detected patch.
- `logger.finalizeRoast {clientRoastId, markers, weights}` — assembles chunks → calls the existing `insertBatch` path with `source:"probe"` (server derives RoR, decimates, depletes inventory if lots are linked, **auto-schedules the AI readout**) → stamps `roastBatches.clientId` → cleans up live/chunk docs (retention: delete after e.g. 48h) → returns the batch id, which the logger stores as `synced_batch_id`.
- Raw session archive: request `storage.generateUploadUrl()` (authed mutation; 1h-expiry signed URL, any HTTP client can POST — do it from Rust) → upload full-res JSON/alog → `logger.attachRawFile {clientRoastId, storageId}` → finally populates the dormant `rawFileId`.
- `logger.importLocalHistory` — first-sign-in bulk path: batches of 50–100 roast summaries through the same idempotent upsert keyed on `clientId`; full-res curves via upload URLs. Resumable (per-row `synced_ms` locally); re-running any batch is harmless. Ownership stamped server-side from `ctx.auth.getUserIdentity()` — never client-supplied.

**Identity & offline semantics**: every roast gets a UUIDv7 at CHARGE + per-install `deviceId`; a roast is captured on exactly one machine, so multi-device conflicts are structurally impossible — sync is append/finalize, cloud is a mirror, local wins. Fully-offline roasts simply flush later (skip live docs entirely if the roast already finished — go straight to finalize + chunks; the companion viewer is moot for a finished roast).

---

## 11. droptime-app backend changes (enumerated)

1. Schema: add `liveRoasts`, `roastSampleChunks` tables; add `clientId?: v.string()` to `roastBatches` + index `by_org_client` (don't overload `fileHash`/djb2 for this — it stays for file imports; also replaces the O(org-batches) linear scan pattern with an index lookup for logger sync).
2. New `convex/logger.ts` mutation family (§10) reusing `insertBatch` — which is currently module-private in roasts.ts, so export it or hoist to lib.ts — including the machine-upsert (+ hobbyist 1-machine enforcement) and coffee-resolution steps.
3. Raw-file path for roasts: `generateUploadUrl` + `attachRawFile`, mirroring the established cupping voice-memo upload pattern (cupping.ts already wraps `ctx.storage.generateUploadUrl()` with permission gating — copy it); finally populates the dormant `rawFileId`.
4. **Hard AI gate for hobbyist — in `recordAiAction` itself** (or a shared `requireAiBudget` helper called by every Claude entry point: `generateReadoutAI`, `askRoastQuestion`, chat, cupping, blend transitions, `regenerateReadout`). Readout path writes a gated row rendered as an upgrade card; other modules return a typed gate error; in-flight Q&A completes the current turn and gates the next. Monthly via `periodYm` is native. Paid tiers unchanged (soft).
5. Sign-in-token endpoint for the desktop handoff (Backend API `createSignInToken`, 60s expiry, nonce round-trip) — lives in droptime-app (Next route handler or Convex httpAction). **Prototyped in Phase 0** — the auth spike cannot run without it.
6. Companion viewer page: `app/(app)/roasts/live/[clientRoastId]` subscribing to the live doc + chunk tail (reuses `@lemon/roast-console` chart in read-only mode). Cheap by construction given the doc shape.
7. Analytics events + a `stale` sweep cron (liveRoasts with `lastSampleAt` > 10 min → status stale; chunk cleanup 48h post-finalize).
8. Note: droptime-app is Next 16 — per its AGENTS.md, read `node_modules/next/dist/docs` before touching app routes.

---

## 12. Website repositioning & logger surfaces (droptime-web)

Positioning updates (Ryan, July 9): **all-in-one tool for coffee roasters of any size** — retire "micro-roasters" framing.

1. **Messaging pass**: homepage h1/eyebrow (currently "Coffee roasting software for micro-roasters"), tagline retained ("A colleague, not a lab instrument" still lands), feature pages, both blog posts' framing, JSON-LD, OG images. Size-inclusive proof points (Phidgets = the commercial-drum standard; MODBUS = Loring/Giesen/Probat; "from your first Skywalker to your production Loring").
2. **`/download` page**: platform-detected download buttons (GitHub Releases artifacts), hardware-compatibility matrix (honest: supported / in progress / **under investigation** per §4 — Bullet is "under investigation," not "coming," until the clean-room project has hardware committed), release notes feed from the updater manifest.
3. **`/demo` page**: the pure-client simulator (§8) — the hero marketing asset. "Watch a live roast right now. No signup, no download."
4. **Nav/CTA split**: primary CTA becomes "Download the free logger" beside the existing cloud-demo CTA (`lib/site.ts` gets `DOWNLOAD_URL`/label constants; Nav, Footer, CtaRow, sitemap updated).
5. Pricing page: add the Logger column (Free, forever, anonymous) feeding into Hobbyist/Roastery/Pro; keep flat-pricing-vs-Cropster stance; do not quote Cropster's gated tier prices.
6. Updater manifests: host per-channel `latest.json` (stable/beta) as static files on the site or a `downloads.` subdomain — tauri-action generates them in CI; a release step copies to Vercel.
7. **Trademark guardrails for the compatibility matrix**: word marks only (no vendor logos); "works with" / "compatible with" phrasing; never "certified/official/partner" absent a written agreement; a footer line that third-party trademarks belong to their owners; no vendor marks in product names or metadata. We're simultaneously soliciting cooperation from one mark owner (Kaleido) and reverse-engineering another's product (Aillio) — the nominative-use defense must stay airtight exactly where it's most needed.
8. **Messaging staging rule**: the "any size" repositioning ships on the same release train as the MODBUS-capable build (§4 row 3 is in the v1.0 ship gate). If MODBUS slips, the copy stages down to wedge-honest ("from your Skywalker mod to your probe-equipped drum") until it lands — the homepage must never promise machines the /download matrix contradicts.

---

## 13. Distribution & ops

- **CI**: tauri-action v1 on tag push — matrix macOS aarch64+x64 / Windows x64 (Linux x64 when tier-2 lands); creates the GitHub Release, uploads installers + `.sig`, generates `latest.json`. `projectPath: apps/droptime-logger`. rust-cache for cargo; turbo builds the Vite frontend.
- **macOS**: Apple Developer Program (**$99/yr — launch prerequisite, enroll week 1**; free accounts cannot notarize). Developer ID Application cert (Account Holder role creates it); `tauri build` auto-signs + notarizes with the App Store Connect API-key env vars (notarytool era). Not sandboxed (outside MAS) — no USB entitlement needed.
- **Windows**: **Azure Artifact Signing** (renamed from Trusted Signing): $9.99/mo Basic, orgs in US/CA/EU/UK, individual developers in US/Canada (self-employed onboarding open since ~April 2026; the preview-era 3-year-history requirement was dropped), requires a paid Azure subscription — **verify eligibility week 1**; fallback = OV cert with SmartScreen cold-start warnings. Wire via `bundle.windows.signCommand`. NSIS installer, `installMode: "passive"`.
- **Updater**: tauri-plugin-updater; signing keypair via `tauri signer generate` (private key in CI secrets — shell env only, .env files explicitly don't work; signature verification cannot be disabled). Static per-channel manifests (§12.6). Linux: updater supports AppImage only — deb users update via package manager (documented).
- **Crash reporting**: tauri-plugin-sentry 0.5.0 (pin) — one Sentry context across Rust (minidumps) + webview JS.
- **Analytics**: posthog-js in the webview — full-bundle import (not the remote snippet; offline-tolerant, queues while offline), `persistence: 'localStorage'` (custom-scheme origins make cookies unreliable). Anonymous by default; identify on Clerk sign-in. Rust-side events (if any) POST /capture directly.
- **Privacy stance for the wedge**: local roast data never leaves the machine unless signed in; analytics are usage-level and disclosed; make this a marketing line — it contrasts with cloud-only competitors.

---

## 14. Phased roadmap

Estimates are focused engineering time, stated as **medians, not ceilings** — §16's spike outcomes (Clerk handoff viability, Convex bandwidth economics, signing eligibility, libphidget22 CI build) are named go/no-go gates that can stretch them. Agentic dev has compressed comparable phases in this repo, but unresolved spikes are exactly what makes estimates floors. **Ship gate for v1.0-public: phases 0–3, including the closed-beta exit.**

**Phase 0 — Spikes & unblocking (~1 week, parallelizable)**
- Scaffold `apps/droptime-logger` (Tauri+Vite) + `packages/roast-console`; prove Channel streaming + SQLite write path with a stub source.
- **Auth spike** (highest technical risk): prototype the sign-in-token endpoint in droptime-app first (§11.5 — the spike can't run without it), then the deep-link handoff + ClerkJS in a bundled dev build, testing the §9 fork: native mode + keyring vs standard-mode fetch-proxy fallback; loopback redirect as the dev/Linux path. Exit criterion: Convex authed round-trip from the installed app via either fork branch.
- **Buy the Phidgets rig** (~$110). Enroll Apple Developer ($99/yr); verify Azure Artifact Signing eligibility. Spike: libphidget22 builds from BSD source in CI for macOS/Windows (§2 bundling premise).
- **Email Kaleido** for protocol docs (machine-sponsor precedent). Post for Bullet R2 community testers. Install RoasTime and read its EULA (§4 provenance item). Begin beta-cohort recruiting (Home-Barista/r/roasting) — testers are needed in Phase 2.5, and recruiting has lead time.
- Generate real .alog fixtures via Artisan simulator mode.

**Phase 1 — The beautiful part (~2–3 weeks): console + simulator, fully local**
- Live console UI on the design system; live-mode chart fork (stable axes, leading-edge follow, trailing RoR, ghost target + delta, phase bars, marker bar, projections); °C/°F display toggle end-to-end.
- Rust capture engine + replay driver; SQLite layer **including the `user_version` migration ladder**; crash recovery ("Resume roast"); post-DROP summary; local history + comparison; .alog/CSV import, .alog/CSV/JSON export.
- **First Developer ID-signed macOS build** as soon as the cert lands — beta distribution and keychain stability both depend on signing, and it must not wait for Phase 3's full CI.
- Exit criterion: a flawless simulated roast end-to-end on all three OSes; force-quit mid-roast recovers losslessly.

**Phase 2a — Real hardware (~1.5–2 weeks)**
- Phidgets driver against the real rig (probe + heat gun = real noise), with our CI-built libphidget22 bundled; TC4 driver against an ESP32 emulator; **generic MODBUS-TCP poller** (no hardware needed for the generic layer; in the ship gate per §4); hotplug + auto-detect.
- **First-run device setup flow + the live failure matrix** (§6): channel-role assignment with live preview, manual port picker, unplug/replug mid-roast, port-held-by-Artisan detection, flat-line banner.
- Exit criterion: the §6 failure matrix passes, not just a happy-path roast.

**Phase 2b — The platform seam (~1.5–2 weeks, overlaps 2a)**
- Convex sync: schema additions, `logger.*` mutations (incl. `upsertMachine` + coffee resolution + 1-machine enforcement), outbox flusher, live-doc/chunk shape, finalize→readout, raw-file upload, first-sign-in import, **hard AI gate in `recordAiAction`**, onboarding path (org + chooseFreePlan before first sync).
- Sign-in flow productionized (keyring, session-expiry degradation, RoasTime-running detection groundwork).
- Exit criterion: a live Phidgets roast appears in app.trydroptime.com with an AI readout; the same roast captured fully offline syncs identically later.

**Phase 2.5 — Closed beta (~1–2 weeks calendar, overlapping)**
- Signed builds distributed through the **beta updater channel** to the 5–10 recruited TC4/Skywalker + Phidgets roasters; device-lab diagnostics page (raw frame dumps testers can send us).
- Exit criteria: ≥10 external real roasts captured cleanly across ≥3 distinct rigs; the logging-only vs needs-control split of TC4 users measured (feeds the §1 "replacement" messaging); no data-loss incident.
- This phase exists because unsigned macOS builds are Gatekeeper-blocked and the TC4 driver cannot be validated internally (hardware retired 2025) — beta distribution cannot wait for public launch.

**Phase 3 — Ship it (~1–2 weeks)**
- Full CI + Windows signing + updater stable channel; download/demo pages; website repositioning pass (§12, honoring the staging rule in §12.8); Sentry + PostHog; docs (device setup per driver, udev rules, Bullet/WinUSB notes).
- Companion viewer page in droptime-app.
- Exit criteria: signed installers on GitHub Releases; auto-update proven stable→stable **including "update mid-history: old roasts intact, outbox drains, migration ladder ran"**; trydroptime.com repositioned with demo + download live.

**Phase 4 — Hardware & surface expansion (ongoing)**
- Kaleido (vendor-gated). Bullet R2 clean-room (hardware/tester-gated + EULA review), then R1. Linux tier-2 promotion. Per-brand MODBUS presets (Loring/Giesen/Probat register maps) on top of the generic poller.
- Candidate v2.x: machine control (explicit decision point — see §1), MQTT ingest, Probat WebSocket-JSON, shareable public roast pages (viral hook), multi-probe rigs beyond BT/ET.

---

## 15. Consolidated risks

| Risk | Severity | Mitigation |
|---|---|---|
| Clerk desktop flow is community-charted territory (official "not planned") | High | Phase-0 spike is the gate; fetch-proxy technique proven by community plugin; loopback fallback; worst case = browser-pasted device code flow |
| Bullet protocol has zero non-GPL documentation; R2 PID mapping unverified | High (for Bullet owners) | Honest compatibility matrix at launch (Bullet = "coming"); own-hardware USB capture project; single-consumer UX (detect RoasTime) |
| Kaleido docs may not materialize | Med | Vendor outreach week 1; sniffing fallback; ship v1 without Kaleido if needed |
| No real .alog fixtures / no hardware during phase 1 | Med | Artisan-simulator-generated fixtures + Phidgets rig purchase + noise-injection simulator; TC4 via ESP32 emulator |
| Windows/Linux webview throttling gaps data if capture leaks into JS | High | Architecture rule #1 (capture in Rust) — enforced by design, tested by minimize-during-roast test |
| Linux webkit2gtk rendering flakiness | Low (tier-2) | Canvas2D/SVG only; env-var workarounds; Linux explicitly tier-2 |
| Azure signing eligibility / Apple enrollment delays | Med | Week-1 enrollment; OV-cert fallback (SmartScreen warnings acceptable for beta channel) |
| Live-viewer bandwidth blowup from wrong doc shape | Med | Small-live-doc + chunk schema from day one (§10); change-detection server-side; metered spike test on Starter plan before publicizing |
| Free-tier AI cost leak (soft metering) | Med | Hard gate in `recordAiAction` across ALL Claude entry points ships in phase 2b, before any public launch |
| Rostoc/HiBean/Firescope contest the niche (Rostoc already ships free desktop capture across much of our driver matrix) | High | Differentiate on local-first + AI readout + console quality + platform upsell — not on capture capability; ship phases 0–3 without scope creep; re-check the field quarterly |
| Positioning outruns hardware coverage ("any size" vs a two-driver launch) | Med | Generic MODBUS in the v1.0 ship gate (§4); messaging staging rule (§12.8) if it slips |
| GPL contamination via careless protocol work | High (legal) | PROVENANCE.md per driver; no Artisan source in the build process; BSD/MIT/vendor/own-capture sources only |
| turbo/Vercel entanglement | Low | Logger's turbo build = frontend only; tauri build in CI only; turbo-ignore already guards the twelve git-connected Vercel projects |

## 16. Spike list (all [med] items + unknowns)

1. Clerk handoff end-to-end in an installed, signed dev build (macOS deep-link constraint makes `tauri dev` insufficient).
2. Bullet R2: confirm 0x0483:0xa27e ↔ R2 mapping and CDC read with `lsusb` + serial probe on real hardware (community tester).
3. Kaleido legacy serial framing (9600/57600; the 115200-8N1 report is from an unverified GPL community bridge — confirm on hardware).
4. Phidgets on macOS: confirm vendor-usage-page HID reads don't trip Input Monitoring TCC.
5. Convex metered spike test: live-doc + chunk shape under 1 viewer × 1 roast on the Starter plan (validate the bandwidth model before marketing "watch live free").
6. Clerk production instance: confirm paid-plan status for raising session max-lifetime past 7 days (trydroptime prod Clerk exists; plan tier unknown).
7. Function-arg limit discrepancy (16 MiB current docs vs 8 MiB older) — size import batches to the conservative 8 MiB.
8. libphidget22 builds cleanly from BSD source in CI for macOS + Windows, and the resulting dylib/DLL bundles + notarizes inside the Tauri app (the §2 premise; phidget-sys links, it does not vendor).
9. RoasTime click-through EULA review (anti-reverse-engineering clause present or absent?) before any Bullet capture work — contract exposure, not copyright, is the live risk there.

## 17. Open decisions for Ryan

1. **Name**: "Droptime Logger" (safe, funnel-aligned) vs a standalone brand. Recommendation: Droptime Logger — the wedge should *build* the Droptime brand, and `trydroptime.com/download` + `downloads.trydroptime.com` need no new domain.
2. **Bullet hardware**: buy a used R2 (~$3.5k new; used market exists) vs community testers only. Recommendation: testers first; buy only if Bullet demand shows up in download telemetry.
3. **Beta motion**: closed beta with 5–10 TC4/Skywalker-community roasters (Home-Barista/r/roasting recruiting post) before public launch — recommended; it doubles as the device-lab validation pool.
4. **Apple/Azure accounts**: enrollments are personal/org decisions (which legal entity signs the binaries).
5. **A low-cost individual tier?** The hobbyist→$79 cliff is steep for home/prosumer users who hit the 20-action gate but will never feel inventory/planning pressure — and that's most of the TC4 launch audience; the paying conversion realistically targets the Phidgets/MODBUS commercial slice, with hobbyists as brand and top-of-funnel. Options: accept that (recommended for launch) or add a ~$10–15/mo "Solo" AI-only tier later. Recommendation: decide after 60 days of gate-hit telemetry; don't build pricing speculatively.

## Appendix: licensing provenance quick reference

| Component | License | Status |
|---|---|---|
| aArtisanQ protocol docs (commands.txt, firmware headers) | BSD-3-Clause (MLG Properties/Gallt) | cite freely |
| libphidget22 ≥ 1.11.20220822 (source) | BSD-3-Clause (older = LGPL-3.0 — pin!) | **build from source in CI**, bundle our artifact + THIRD-PARTY-NOTICES attribution |
| Phidgets prebuilt MSI / macOS Framework | Phidgets EULA (no transfer except with hardware) | **never redistribute** |
| `phidget` Rust crate | MIT (Pagliughi, unofficial, production-intent) | use (links our lib via PHIDGET_ROOT) |
| tokio-modbus, serialport, nusb, hidapi, rusqlite, py_literal | MIT/Apache | use |
| Artisan (incl. aillio.py, kaleido.py) | GPL-3.0 | **never read by an implementer**; spec-writer access only inside the §4 clean-room SOP; prefer own-capture paths |
| .alog file format | unprotectable as expression (EU: SAS v. WPL; US: §102(b)/merger analysis — no square US format holding, don't overclaim) | parse/write from own sample files + issue-thread schema facts (never Artisan source); PROVENANCE.md applies |
| Kaleido protocol | vendor-proprietary, cooperation precedent | email first |
| Aillio protocol | undocumented publicly | own-traffic USB capture or 2-person clean room |
