# Droptime Logger — Hardware Expansion Scoping (post-v0.1.0)

> **Kaleido outreach: SENT July 11, 2026** via kaleido contact form (protocol docs + loaner ask, co-marketing offer, read-only + clean-room framing). Clock started — if no reply within ~2 weeks, follow up via their sales email and the Artisan-community contact path. The driver build stays gated on their response per §Kaleido.

**Prepared:** July 10, 2026. **Inputs:** four verified scoping reports (July 10) on the next driver targets — Phidgets, generic MODBUS, Kaleido, Aillio Bullet — each calibrated against the shipped TC4 datapoint: **~1 focused agent-day** for driver + sniffer + preview + auto-detect + pty emulator + full test suite, achieved *because* the protocol was BSD-documented and emulator-buildable without hardware. Every estimate below is priced against that anchor. `droptime-logger.md` (strategy) and `droptime-logger-oss-v0.1.0.md` (release plan) stay in force; this doc scopes the v0.2 train and the vendor-gated tail, and folds in five corrections the research surfaced (§7).

**The one-line thesis:** the two drivers the README already promises for v0.2 (Phidgets + MODBUS) are also the two cheapest, cleanest, and most parallelizable — ~5–6.5 agent-days combined, ~$137 of hardware, zero clean-room exposure, zero vendor dependency. Everything expensive (Kaleido, Bullet-live) is gated on *other people* — a vendor email and a demand signal — so the correct move is to fire those long-lead triggers **today** and build the v0.2 pair while they cook.

---

## 1. Executive answer — what each target takes

**Phidgets (VINT hub + TMP1101) — ~2–3 agent-days, ~$112 hardware, gated by CI packaging, not protocol.** There is no protocol to reverse-engineer: we link the vendor's own BSD-3-Clause `libphidget22` (source is BSD since v1.11.20220822; only the prebuilt MSI/Framework installers carry an EULA) and it hands us typed °C values via callbacks over its own libusb/HID transport — invisible to `serialport`, so this is a new discovery branch, not a serial variant. The driver adapter is *simpler* than TC4 (the library subsumes framing, polling, and reconnect); the real cost is FFI + building/bundling libphidget22 from source across three OSes in CI, including one sharp, hardware-independent edge: `phidget-sys` links a `.framework` on macOS and ignores `PHIDGET_ROOT`, while our source build yields a dylib — we must repackage-as-framework or fork its `build.rs`. Weakest no-hardware test story of the clean targets (no pty trick; trait-boundary mock is the primary rig). Zero clean-room risk, zero vendor calendar risk.

**Generic MODBUS (TCP + RTU) — ~2.5–3.5 agent-days, ~$25 hardware, gated by nothing.** Fully open standard, `tokio-modbus` (MIT/Apache, maintained) does framing/CRC/MBAP for both transports and ships in-process TCP/RTU **servers** — so the emulator is *easier* than TC4's pty hack and runs on Windows CI. Read-only by construction (FC03/FC04 only; write codes structurally absent). The generic TCP layer needs no hardware at all; a ~$13 FTDI RS-485 dongle plus a ~$10 XY-MD02 sensor validates real RTU wire behavior. This is the v1.0 ship-gate item that makes "any size / works with Loring" honest. The only multi-week item is sourcing a **Loring-owning beta roaster** for the preset's register map (own-capture, never Artisan source) — calendar risk that gates the *preset*, not the engine. **Correction folded in from the research: Giesen is Siemens S7comm, not MODBUS — this driver opens the Loring + Probat door, not the Giesen door (§7).**

**Kaleido — ~2.5–3 agent-days if the vendor sends the doc (+1 day hardware validation), ~4–6 days and hardware-gated if not; ~$1,199–1,424 for a dev unit unless we get a loaner; gated by one email.** Kaleido co-developed its protocol integration with Artisan (machine/protocol sponsor — stronger precedent than "gave money"), so the outreach prior is the best in the matrix. But no public byte-level spec exists; the only public implementation is Artisan's GPL `kaleido.py` — clean-room poison. A vendor doc collapses Kaleido to TC4-difficulty (documented ASCII-ish serial at 9600 8N1 over CP210x — a chip `serial.rs` already classifies — plus a TCP network mode needing only a small `Transport: Read+Write` seam, no new crates). Without the doc, everything serializes behind a ~$1,200 purchase and an own-capture campaign. Therefore: **email first, buy later, and ask for a loaner/dev discount in the email.** Ship China→Thailand is the favorable route (~1–2 weeks) if we do buy.

**Aillio Bullet — live driver: ~8–15 agent-days gated behind ~$4,099 hardware + a multi-week clean-room capture campaign; stopgap importer: ~1–2 agent-days, $0, gated by nothing.** No permissively-licensed protocol description exists in any language; the only byte-level knowledge is GPL `aillio.py`. Transport is raw WinUSB/libusb **bulk transfers** (via `nusb`, not `serialport` — no TC4 infra reuse), binary framing, firmware-keyed transport regimes even within the R1 line, an exclusive-device conflict with RoasTime, and a RoasTime click-through EULA that must be read *before* any capture (contract risk > copyright risk). The cheap, clean path that exists **today**: Aillio's official **RoastWorld public REST API v3** (Dec 2025, `x-api-key`) for post-roast import — real Bullet-owner value, zero hardware, zero clean-room, zero USB conflict, and every import is a hard telemetry signal for whether the live driver is worth $4k + weeks. Ship the importer; let demand decide the rest.

---

## 2. Recommended sequence

**Ordering logic, stated once:** (a) the README's v0.2 promise is Phidgets + MODBUS — breaking the first public promise the project makes is not on the table; (b) the wedge audience (TC4/Skywalker DIY + probe-equipped drums) is served by exactly those two drivers — Phidgets *is* the "don't have a digital roaster?" wizard path and the commercial-drum retrofit rig, MODBUS is the "any size" credibility gate; (c) vendor-cooperation lead time (Kaleido ~1–3 weeks round-trip, Loring beta-owner weeks) is pure calendar that costs nothing to start and everything to delay; (d) almost everything is parallelizable *if* the long-lead triggers fire on day 0.

### Day 0 — fire every long-lead trigger (Ryan, ~1 hour, ~$137)
1. **Send the Kaleido email** (talking points in §3). Free, longest lead time on the board, and a "yes" converts Kaleido from a hardware-gated clean-room project into a TC4-difficulty documented build.
2. **Order the Phidgets rig** (~$112, SKUs in §3) — the only hardware-gated item on the v0.2 train.
3. **Order the MODBUS RTU rig** (~$25) alongside it.
4. **Post the Loring beta-owner recruit** (Home-Barista / r/roasting, the device-report campaign from the v0.1.0 plan §5.3): one owner enables the on-panel "Artisan" MODBUS-TCP mode and sends a raw register capture — the only artifact the software can't manufacture itself.
5. **Read + log the RoasTime EULA** (PROVENANCE entry) — hours, but it hard-gates any future Bullet capture.

### Track A (agents, starts immediately, no hardware): the v0.2 pair — ~5–6.5 agent-days
1. **Generic MODBUS-TCP first** (~1.5 days to a working source against the in-process `tokio-modbus` tcp-server emulator; ~2.5–3.5 with RTU + wizard + decode-matrix tests). Zero external dependencies; de-risks the v0.2 tag date all by itself.
2. **Phidgets in parallel** — lead with the two hardware-independent spikes: the **macOS framework-vs-dylib fix** and the **libphidget22 CI build-and-bundle × 3 OSes** (~1–1.5 days, the real cost), plus the trait-boundary-mocked adapter (~1 day). Real-rig validation slots in whenever the box arrives.
3. **v0.2 ships when both are emulator/mock-green and Phidgets has passed the real rig.** MODBUS presets (Loring +0.25 day, Probat +0.25) ride whatever release train follows the register maps — they are data tables on the generic engine, not gates.

### Track B (agents, ~1–2 days, anytime): Bullet RoastWorld importer
Ship post-roast import via the public API v3 as the v0.x Bullet story — converts "/download: under investigation" into shipped value and instruments the buy-a-Bullet decision (plan §17.2). Reuses the `.alog`/history import plumbing.

### Track C (calendar-gated): Kaleido
- **Vendor replies with doc** → Path A build (~2.5–3 agent-days: serial source + `Transport` seam + TCP variant + emulator twins + wizard branch), emulator-validated before any hardware arrives; buy (or accept loaner) M1 for ~1 day of bench validation. Realistic wall-clock ~3–6 weeks end-to-end, dominated by email + shipping, not code.
- **Vendor silent after ~3 weeks** → decide whether to fund Path B (buy M1 ~$1,200, own-capture campaign, ~4–6 agent-days serialized behind delivery) or park Kaleido at "under investigation." Recommendation: park unless launch telemetry shows Kaleido owners in the funnel — the same demand-gating logic as the Bullet.

### Explicitly sequenced last: Bullet live driver
Only after importer telemetry proves demand. Then: buy R2 (~$4,099 new, ~$3,000–3,500 used), software-capture first (usbmon/USBPcap, $0), Beagle USB 12 (~$400–500) only if software capture drops packets. ~8–15 agent-days behind 2–4 calendar weeks.

---

## 3. Ryan's action list

### Purchases (place all day 0)

| Item | SKU | Price (USD) | Ship-to call |
|---|---|---|---|
| Phidgets VINT Hub 6-port | **HUB0002_0** (HUB0000 is discontinued — §7) | $40.00 | Phidgets ships from **Calgary**: Toronto ~2–4 business days, ~$15 CAD, no customs; Thailand ~1–2 weeks via UPS Global Checkout + duty/VAT. **Ship to whichever city you'll physically be in when the Phidgets work lands; default Toronto.** Amazon US Phidgets store is the slightly-pricier Prime-fast fallback for North America |
| Phidgets 4× thermocouple module | **TMP1101_1** | $40.00 | same box |
| K-type probes ×2 | **TMP4106_0** | $16.00 ea | same box; confirm the 10 cm VINT cable is bundled with the TMP1101 |
| **Phidgets subtotal** | | **≈ $112** | matches the plan's ~$110 estimate |
| USB↔RS-485 dongle | **DSD TECH SH-U11** (FTDI FT232R; Amazon `B07B416CPK`) | ~$13–15 | Amazon 1–5 days to a North-American address; FTDI VID `0x0403` already auto-classified by `serial.rs` |
| Real MODBUS-RTU slave | **XY-MD02** SHT20 transmitter (Amazon `B0CYLWKS76`; also Newegg/AliExpress) | ~$6–12 | a $10 stand-in for a $40k roaster's RTU behavior (FC04, int16 ÷10, unit-id 1); AliExpress is the workable route if ordering to Thailand |
| **MODBUS subtotal** | | **≈ $25** | |
| Kaleido Sniper M1 "Pro" Artisan System (200g, direct USB/CP210x) | kaleido-sniper.com / kaleido-coffee.com (Wuhan factory-direct) or coffeeroastco.com | ~$1,199–1,424 (sale; list ~$1,800–2,600) | **DO NOT BUY YET.** Gate on the vendor thread: a doc may make the build emulator-validatable before hardware, and the email asks for a loaner/discount. If buying: China→Thailand is the favorable route (~1–2 weeks, intra-Asia). Skip the Dual/WiFi unit (+$340) — network transport is not v1 |
| Aillio Bullet R2 | us.aillio.com | $4,099 (used ~$3,000–3,500) | **DO NOT BUY.** Gated on importer telemetry (§2 Track B / plan §17.2). Not R2 Pro ($5,299 — roasting features, same protocol) |
| Beagle USB 12 analyzer | Total Phase | ~$400–500 | **DO NOT BUY** unless free software capture (usbmon/USBPcap) proves lossy |

### The Kaleido email (send today — to Wuhan Kaleido Technology via kaleido-coffee.com / kaleido-sniper.com contact)

Lead with the cheap "yes" (the doc) and the shared-ethos framing:
- **We're building the next free, open-source roast logger (AGPL — same spirit as Artisan) and want Kaleido first-class at day one.** Open project, not a proprietary product extracting value from their ecosystem.
- **"You already did this once, with Artisan."** Same ask they granted there: the serial + network protocol spec — command/response framing, baud per generation (9600 Board-C / 57600 legacy), BT/ET/AT/SV/heater/fan/drum semantics, WLAN handshake. For them it's forwarding a doc they already have.
- **More free software that says "works with Kaleido" = more Kaleido sales** — the same flywheel that presumably motivated the Artisan sponsorship, at zero cost to them.
- **Read-only first, hardware-safe by design** — v1 never sends heater/fan/drum/setpoint; we know the Dual's USB is tablet-reserved and mis-wiring can damage the control board. Control is a later, explicit, opt-in decision.
- **Co-marketing / design-partner offer** — supported-machine listing (nominative "works with," word mark only), early testing against new models, high-quality bug reports.
- **The ask:** Board-C protocol doc for M1/M2/M6/M10 (legacy dual if handy). **Bonus ask:** loaner or dev-discount M1 — we're in Thailand, shipping from the factory is easy — but the doc is what unblocks us.

### Other contacts / gates to initiate now (all have lead time)
- **Loring beta-owner recruiting post** (Home-Barista + r/roasting) — the register map is the only true multi-week dependency in the MODBUS effort. Optionally email Loring (and Probat) for register maps too: days–weeks latency, near-zero cost.
- **RoastWorld API key** — register for the public API v3 (`api.roast.world/api-docs`) so the importer work isn't blocked on account plumbing.
- **RoasTime EULA** — install, read, log in PROVENANCE (plan spike #9). Gates all future Bullet capture; prefer EU/UK-jurisdiction captures or volunteer testers if it carries an anti-RE clause.
- **Aillio outreach** — optional, low probability, weeks of latency (they route developers to the cloud API). Send if free, but **never on the critical path**.

---

## 4. Per-target build plans

Calibration anchor throughout: TC4 = ~1 agent-day against a documented protocol, no hardware, full test suite via pty emulator.

### 4.1 Phidgets (`capture/phidgets.rs`) — ~2–3 agent-days

- **Driver shape:** `PhidgetSource: DeviceSource`, structurally simpler than TC4. `start()` opens one `TemperatureSensor` per assigned channel (`set_hub_port`/`set_channel`/`set_serial_number`, `open_wait` fast-fail → `port_error`), sets K-type + 1000 ms data interval; **events not polling** — temperature-change handlers build `SampleDto`s (°C→°F via `to_fahrenheit`), attach/detach handlers forward as `Status { Disconnected|Reconnected }` (libphidget22's internal auto-reconnect replaces our 3-strike machine; the *contract* — Disconnected-before-Reconnected, gap-free seq — is kept, the mechanism delegated). New logic: a latch/coalesce debounce folding per-channel events (up to 50 Hz) into one `SampleDto` per 1 Hz engine tick. Open-TC/non-finite rejection ports over from TC4 (never bind NaN→NULL).
- **Stack:** `phidget` crate **0.4.0** (MIT, thin FFI, single maintainer — **pin and be ready to vendor**) over `phidget-sys` linking our CI-built libphidget22. We never open a COM port and never call hidapi ourselves.
- **Emulator/test strategy (the weak link — no pty trick exists):** (1) *primary:* trait-boundary mock (`ThermocoupleChannel`) so mapping/latch/attach-detach/open-TC logic is fully unit-testable with zero FFI; (2) *optional:* mock Phidget Network Server (port 5001, documented) exercising the real library over loopback — defer unless the FFI seam leaks; (3) real rig = the only test for HID attach, macOS Input-Monitoring TCC behavior, and notarized bundling. Probe + heat gun = real noise.
- **Wizard integration:** a new top-level "Phidgets (USB)" branch — `PhidgetManager` enumeration replaces `list_ports()` (nothing to sniff; the library self-identifies "TMP1101 on hub port N"), then the existing channel-role + live-preview UX verbatim, fed from temperature events. `SourcePinDto` gains a hub-port/serial locator. Linux: udev rule (VID `0x06c2`) in the .deb, documented for AppImage.
- **Clean-room guardrails:** essentially nil — BSD source + MIT crate, no GPL anywhere, no PROVENANCE SOP needed. The single discipline: **build libphidget22 from the BSD source tarball in CI; never redistribute the EULA'd prebuilt MSI/Framework.** THIRD-PARTY-NOTICES gets the BSD attribution.
- **The critical path is CI (~1–1.5 of the 2–3 days):** autotools build (needs libusb-1.0-dev) × Linux/Windows/macOS, bundle into the Tauri app, notarize (sign the dylib, fix rpath), and resolve the **macOS framework-vs-dylib wrinkle** — repackage our dylib as `Phidget22.framework` or fork `phidget-sys`'s `build.rs` (it honors `PHIDGET_ROOT` on Linux/Windows only). Spike this first; it is 100% hardware-independent.

### 4.2 Generic MODBUS (`capture/modbus.rs`) — ~2.5–3.5 agent-days

- **Driver shape:** new `modbus-tcp:`/`modbus-rtu:` prefixes in `build_source`; the TC4 read-loop skeleton (interruptible `wait_until`, poll timer, 3-strike → Disconnected → 2s–30s doubling backoff → Reconnected, gap-free seq, finiteness guard) transfers directly — only *how one poll happens* (read N registers via `tokio-modbus` `sync` client, keeping tokio out of the capture engine) and *how a frame decodes* change. New `ModbusPinDto`: transport (TCP host:port-502 | RTU port/baud/parity), unit-id, byte order, per-channel `{role, FC03|FC04, address, decode: uInt16…Float32|BCD16|BCD32, divisor 1|10|100, unit}` — batched into contiguous reads where possible. **Read-only by construction:** only `read_holding_registers`/`read_input_registers` exist in the type; no write FC in the code at all.
- **Decode matrix is the real work:** 7 decoders × endianness × divisor + sign extension, table-driven unit tests; BCD16/32 (the Loring path) is the historically fiddly one.
- **Emulator/test strategy — easier than TC4:** in-process `tokio-modbus` **tcp-server** on `127.0.0.1:0`, register map seeded from a replayed roast curve; drop the listener to exercise the disconnect matrix. No `#[cfg(unix)]` — runs on Windows CI. RTU path: the existing pty escape hatch (`open_port` `baud==0`) + `rtu-server`, plus the XY-MD02 for real wire/CRC fidelity. A Loring register-map JSON fixture doubles as emulator seed and preset test data.
- **Wizard integration:** MODBUS is polled request/response — **passive sniffing doesn't apply**, so no `SniffVerdict` extension. Instead: guided config + "Test connection" probe reusing `start_preview`/`PreviewEvent` — user confirms "register 18 = 412°F, that's my BT" live. TCP auto-detect is cheap (probe Loring's fixed IPs `.199`/`.69` on :502, offer the preset on answer); RTU has no auto-detect by nature — serial enumeration (FTDI/CP210x/CH340 already classified) + user-supplied baud/unit.
- **Clean-room guardrails:** protocol layer is fully open — the **only trap is register values**. Preset maps come from a beta owner's own-hardware capture or the vendor directly — **never transcribed from Artisan's GPL machine-configs** (`loring.py`, `.aset` files). Artisan's MODBUS *documentation page* (the config-surface feature list) is fair to study — it's product docs, not source; keep the two provenance trails separate in each preset's PROVENANCE.md.
- **Generic-first beats presets-only:** endianness, register base conventions, int/BCD/float, ÷10/÷100 all vary by machine *and firmware* — a slightly-wrong preset is worse than a live "Test read." Presets (Loring first: pure TCP, fixed IPs, owner-enabled "Artisan" mode, no paywall; Probat needs a Pilot-equipped owner) are +0.25 day each once a map is in hand.

### 4.3 Kaleido (`capture/kaleido.rs`) — ~2.5–3 agent-days (Path A) / ~4–6 (Path B)

- **Driver shape:** `KaleidoSource` as a sibling of `Tc4Source` — same synchronous fast-fail open, named read thread, 1 Hz cadence, timing struct, 3-strike/backoff/gap-free-seq machine, NaN guard. Channels map 1:1 onto `ChannelKind { BT, ET, Ambient, Heater, Fan, Drum }` + `Aux` for SV. **Closed read-only command enum** mirrors `ReadOnlyCommand`: only the poll/read verbs exist; heater/fan/drum/SV control verbs are structurally absent. The exact verb strings are the one unknown the vendor doc (or capture) supplies.
- **The one real refactor:** a small `Transport: Read + Write + Send` seam so `LineReader`/`init_device`/`poll_once` work over both `Box<dyn SerialPort>` and `TcpStream` (~0.5 day) — serial (9600 8N1 Board-C / 57600 legacy, CP210x already classified `likely`) and WLAN-network share the framing. No new crates; `mdns-sd` discovery deferred, manual host:port for v1 — and **don't commit v1 to WLAN at all**: its framing is genuinely undocumented publicly.
- **Emulator/test strategy:** the TC4 pty emulator ports directly — `KaleidoEmulator` answers the read verb with synthetic BT/ET/AT/SV/heater/fan/drum frames on a curve, plus a TCP-listener twin for the network transport. The entire TC4 test matrix transfers (units, unplug/reconnect, stays-read-only audit, NaN→disconnect, resume, sniffer, silence). **The blocker is knowing the frame bytes to emulate, not test infra** — with the doc, emulator + suite is hours.
- **Wizard integration:** extend `SniffVerdict` with `Kaleido` (frame-signature after a read probe, alongside TC4 CSV detection); network path gets a manual host:port branch with the same live preview.
- **Clean-room guardrails (the defining risk):** implementing agent's context must **never contain `kaleido.py`** (or the Rostoc app internals). Sources: vendor doc, or own-hardware captures of Kaleido-app/Artisan↔machine traffic (own-traffic facts are clean; Sega/Connectix line; EU Art. 5(3)) — sniffing what our machine emits under Artisan is fine, *reading Artisan source to interpret the bytes* is not. PROVENANCE.md records every input. **A vendor doc collapses this risk to zero — which is why the email is the whole game.** Scope v1 to Board-C serial; label legacy-57600 and network "under investigation" in the /download matrix.
- **Path split:** Path A (doc): serial source ~1 day + transport seam/TCP ~0.5 + emulator/tests ~0.5 + sniffer/wizard ~0.5 = **~2.5–3 days**, emulator-validated pre-hardware, +~1 day bench validation on the M1. Path B (no doc): +1–2 days capture-rig/analysis and **everything serializes behind the ~$1,200 unit** — net ~4–6 days. Wall-clock either way ~3–6 weeks, dominated by email + shipping.

### 4.4 Bullet — importer now (~1–2 agent-days), live driver later (~8–15)

- **Importer (Track B):** RoastWorld public API v3 client (`x-api-key`, `GET /api/v3/public/roast` + per-roast detail, ~2 Hz curves) → roast-profile→schema mapper → import UI, reusing the `.alog`/history import plumbing. Post-roast only (no live stream exists; RoasTime has no local API). Zero hardware, zero clean-room, zero USB conflict; sole dependency is Aillio keeping the API open. Every import is demand telemetry for the live driver.
- **Live driver, when funded:** `BulletSource` over a new **`nusb` (0.2.x) bulk-transport module** — claim interface, kernel-driver detach on Linux, timed bulk reads, and WinUSB error mapping (map "no WinUSB/access denied" to Zadig guidance the way `map_open_error` maps busy→Artisan). Binary frame decoder from **our own captures** (positional/tolerant against firmware variance, same finiteness discipline). **RoasTime-conflict detection is a first-class UX state** (exclusive device; claim-fail/process-scan → plain-words banner). Read-only invariant re-proven via the emulator's command audit log — the Bullet accepts control commands, so the closed-enum discipline matters more here than anywhere.
- **Emulator:** you cannot pty-emulate USB bulk. `BulletTransport` trait seam — production impl nusb, test impl a "fake Bullet" synthesizing captured frames (plus pcap replay fixtures). **Blocked on the first capture session** — unlike TC4, tests cannot precede the hardware.
- **Clean-room guardrails (strictest in the matrix):** GPL `aillio.py`/`mikefsq` fork are poison; no permissive implementation exists anywhere; Aillio's own GitHub org contains the app, not a spec. Own-capture only (usbmon/USBPcap of RoasTime↔Bullet on our machine); **read the RoasTime EULA first** — the live exposure is contract (anti-RE clause, Bowers v. Baystate), not copyright; prefer EU/UK-jurisdiction captures or volunteer testers. PROVENANCE.md throughout; implementing session provably Artisan-free.
- **Estimates:** transport 1.5–2.5 d, decoder 1–2 d, emulator+tests 1.5–2.5 d, wizard/conflict UX 1–2 d, **plus 3–6 d of capture analysis across multiple hot roasts** — ~8–15 agent-days behind 2–4 calendar weeks and ~$4,100. R1 coverage is an additional capture+parse cycle (its own firmware regimes — v626+ changed transport *within* the R1 line), not a freebie.

---

## 5. Risks

| Risk | Target | Sev | Mitigation |
|---|---|---|---|
| macOS framework-vs-dylib linking (`phidget-sys` ignores `PHIDGET_ROOT` on mac) | Phidgets | **High — blocks the primary platform** | Hardware-independent; spike first: repackage dylib as `Phidget22.framework` or fork `build.rs` |
| Accidentally bundling the EULA'd prebuilt Phidgets installer | Phidgets | High (licensing) | CI builds from BSD source tarball only; enforce in the release workflow |
| `phidget` crate bus-factor (single maintainer, 4 stars) | Phidgets | Low | Pin 0.4.0; thin wrapper over stable C ABI — vendor/fork if needed |
| macOS Input-Monitoring TCC silently denying HID open | Phidgets | Med | Only testable on real signed build + rig; vendor-usage-page HID shouldn't prompt |
| Giesen shipped as a MODBUS claim (it's S7comm) | MODBUS / positioning | **High — /download matrix would contradict reality** | Fix plan copy now (§7); S7 is a separate future driver; "any size" copy names Loring/Probat |
| Register maps transcribed from Artisan GPL configs | MODBUS | High (clean-room) | Own-capture via beta owner or vendor only; per-preset PROVENANCE.md; UI-parity facts from Artisan's *docs page* only |
| Loring beta-owner sourcing drags (the true MODBUS critical path) | MODBUS | Med | Generic engine ships without presets; recruit post fires day 0; optional vendor emails |
| Probat network reads paywalled (Pilot option) | MODBUS | Low | Matrix note: "requires Probat Pilot network option" |
| Kaleido stays silent → clean-room + $1,200 hardware gate | Kaleido | Med | Email day 0; 3-week decision point; park at "under investigation" unless funnel telemetry says otherwise |
| GPL `kaleido.py` contaminating the implementer | Kaleido | High | Vendor-doc path preferred; own-capture fallback; context provably clean; PROVENANCE.md |
| Kaleido protocol variance (9600 vs 57600, serial/network/legacy framing) | Kaleido | Med | Scope v1 to Board-C serial; label the rest "under investigation" |
| RoasTime EULA anti-RE clause (contract > copyright exposure) | Bullet | **High — gates all capture** | Read + log before any capture; EU/UK captures or volunteer testers if adverse |
| Bullet firmware/transport variance (v626+ regime change within R1) | Bullet | High | Per-regime captures; tolerant parsing; R2-only first |
| RoasTime single-consumer conflict; WinUSB/Zadig onboarding tax | Bullet | Med | First-class detected state + plain-words UX; Zadig guidance in errors |
| RoastWorld API dependence (importer) | Bullet | Low | Cheap to build; if closed, we lose a stopgap, not a core path |
| Sequencing risk: v0.2 promise slips because effort leaks to vendor-gated targets | Program | Med | Kaleido/Bullet-live are calendar-triggered, not build-tracked, until their gates open |

---

## 6. What we deliberately do NOT build yet

- **Bullet live USB driver** — gated on importer telemetry (plan §17.2). No R2 purchase, no capture campaign, no Beagle analyzer until demand shows.
- **Kaleido WLAN/network transport in v1** and the **legacy-57600 firmware profile** — undocumented/secondary; Board-C serial first, rest labeled "under investigation."
- **Kaleido hardware purchase before the vendor thread resolves** — the email may yield a doc (build pre-hardware) and/or a loaner.
- **Giesen S7comm driver** — a separate protocol, a separate future scoping; never marketed under the MODBUS umbrella.
- **Probat preset** — until a Pilot-equipped beta owner exists (the machine-side paywall is theirs, not ours).
- **MODBUS RTU auto-detect** — physically infeasible (baud/unit/register unknowable); the wizard's live "Test read" is the answer, permanently.
- **mDNS discovery (Kaleido network) / mock Phidget Network Server** — nice-to-haves behind manual host:port and the trait-mock respectively; build only if the cheap path proves insufficient.
- **Machine control, on every target** — the read-only invariant stands (closed command enums, audit-logged emulators). Control remains an explicit v2 decision point, never a driver side-effect.
- **Bullet R1 coverage** — a second capture+parse cycle after R2, if ever.

---

## 7. Corrections to fold back into the existing plans

1. **Giesen is Siemens S7comm, not MODBUS** (Artisan and Cropster both confirm). `droptime-logger.md` §4 row 3 and §12.1 fold Giesen into the MODBUS driver — fix the copy: MODBUS honestly covers **Loring + Probat + generic PLC roasters**; "any size" phrasing must not name Giesen until an S7 driver exists.
2. **Kaleido sponsorship precision:** Kaleido & Beanseeker was Artisan's *machine/protocol* sponsor (co-developed the integration); the *financial* release sponsor of v2.8.4 was BC Roasters/Buckeye. Strengthens (not weakens) the outreach prior — encode the 9600/57600 generation split and the serial/network/legacy transport triple as protocol-variance facts.
3. **HUB0000 is discontinued** — the plan's Phidgets row (HUB0000/HUB0001) should read **HUB0002_0** as the current mainstream hub.
4. **`phidget` crate is 0.4.0**, not 0.4.1 (no such release), and `phidget-sys` **ignores `PHIDGET_ROOT` on macOS** (links a Framework) — the plan's "point phidget-sys at our build" premise holds on Linux/Windows only.
5. **The "R2 = CDC serial / R1 = HID" prior is wrong in practice:** both are driven via WinUSB/libusb raw bulk transfers (vendor-specific protocol), and R1 firmware v626+ changed transport *within* the model line — plan `nusb`, not `serialport`, for any future Bullet work.
