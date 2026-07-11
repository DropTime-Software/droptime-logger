# Droptime — Live Roast Logging: Research Findings & Build Plan

> **⚠️ SUPERSEDED (July 9, 2026):** the tier ship-order below is obsolete. Strategy pivoted to a standalone free desktop wedge product ("Droptime Logger", Tauri) with native capture only — no Artisan relay (Tier 1 dropped), no browser Web Serial capture (Tier 2 dropped), Tier 3's native app promoted to the product itself. See **`droptime-logger.md`** for the current plan. The research facts in §1 (hardware matrix, sampling norms, GPL constraints, browser-capture landscape) remain valid and are cited there.

**Prepared:** July 2026. Research method: 5-angle web sweep → 20+ sources fetched (mostly primary: Artisan source/docs/releases, Cropster/RoastLog/Aillio official docs, WebKit/Chrome standards positions, roaster-community threads) → 115 claims extracted → 22 adversarial verification votes ran, **0 refutations**. Confidence labels below: **[high]** = primary-source documented, **[med]** = inferred or single-source, **[low]** = speculative.

---

## 1. What the research established

### Sampling & data-model requirements (the easy part)
- Industry sampling sits at **1–5 s per point**: Cropster defaults to 1 s; Artisan defaults to 2 s (since v2.6.0) and its author recommends 3–5 s as fully sufficient. **[high]** → Convex can trivially absorb this (a batched mutation every few seconds).
- RoR is the first derivative of BT and is noisy at short intervals; **the correct data model is Artisan's: persist RAW samples, apply smoothing only at display time.** **[high]** → This matches Droptime's existing storage exactly (raw points in file storage + downsampled inline arrays; RoR computed in the chart layer). No schema rework needed.
- Nobody in the industry ships browser-native capture. Cropster = native desktop app (RI) + their Databridge hardware (4-channel TC/RTD) + direct PLC Ethernet for Loring/Giesen; RoastLog = proprietary USB/BT hardware bridge + Phidgets; Aillio = RoasTime desktop over libusb/WinUSB. **[high]** The competitive norm is a local bridge — which means browser-native capture is a *differentiator*, not table stakes.

### The device landscape (what a micro-roaster actually has)
| Ecosystem | Protocol reality | Browser-reachable? |
|---|---|---|
| **Phidgets bridges** (the de-facto probe standard — supported by Artisan, Cropster, RoastLog) | USB; legacy 1046/1048 are **USB HID needing kernel extensions** [high] | **No** (HID + kext = hostile to WebUSB/WebHID) |
| **Aillio Bullet R1** | Vendor-class USB, reverse-engineered, Zadig/WinUSB friction on Windows, **single-consumer only** (concurrent clients crash) [high] | No (vendor-class) |
| **Aillio Bullet R2** | Enumerates as "Bullet R2 CDC" → likely **serial-class** [med — inferred from device string] | **Likely yes via Web Serial** [med] |
| **Kaleido** (incl. Sniper) | Serial 9600/57600 + WiFi/network; WebSocket-native ecosystem; vendor cooperates with Artisan [high] | **Yes** (serial gen via Web Serial; network gen via plain WS) |
| **TC4/Arduino bridges** | USB serial [high] | **Yes** |
| **Skywalker/ITOP stock** | Bit-banged GPIO, no digital protocol; needs a DIY Arduino TC4-emulating bridge; 10 s hardware watchdog [high] | Only via the DIY bridge (then = TC4 serial) |
| **Loring/Probat/Giesen smalls** | PLC: MODBUS (4 variants)/Siemens S7/Ethernet; Probat G/UG 2026 speak WebSocket-JSON [high] | Network-reachable, not browser-serial |

### Browser-native capture maturity (2026)
- **Web Serial: stable on desktop Chromium since Chrome 89** (~74.5% global share), with persistent permissions (`getPorts()` re-connects without re-prompting) and connect/disconnect events — the primitives for multi-hour resilience exist. **[high]**
- **Safari: never.** WebKit formally closed its position as *oppose* (May 2026), explicitly covering Web Serial, WebUSB, and Web Bluetooth. **[high]** Firefox 151 just shipped it but extension-gated **[high]**; Android has no USB serial path. → Browser capture = **desktop Chrome/Edge only**, forever. Acceptable: that's what a roastery laptop runs.

### Artisan-as-bridge (the sleeper finding)
- Artisan's *WebSocket device* is a **client** that polls external servers — it ingests, it does not serve. **[high]** But two real outbound paths exist:
  1. **Per-sample outbound push [high]:** Artisan can be configured (Config ≫ Events) to emit a custom WebSocket `send()` **at every sampling step**, with variable substitution (`{BT}`, `{ET}`, …), producing JSON like `{"bt": 190.8, "id": 9140, "roasterID": 0}`. Documented by the maintainer; sentinel `-1` outside active roasts must be handled. The whole setup ships as a distributable **`.aset` settings file**.
  2. **MQTT outbound (Artisan v4.2.0, June 30, 2026) [med]:** brand-new publish-to-broker IO command — promising v2 path, too fresh to bet the MVP on.
- Stability caveats: the WS plumbing was rewritten (asyncio) in v3.0.0, release cadence is fast → pin tested Artisan versions. **[high]**
- Licensing caveat: Artisan is **GPL-3.0**. Its `aillio.py` even runs headless as a standalone Bullet telemetry monitor — useful as *reference*, but no GPL code may be embedded in Droptime's proprietary bridge. Clean-room reimplementation or run-Artisan-alongside only.

---

## 2. Architecture recommendation: three tiers, shipped in this order

### Tier 1 — "Droptime Link" via Artisan outbound push (MVP, ~2–4 weeks)
Roasters keep their exact rig: Artisan already speaks to *everything* (80+ brands, 200+ configs). We ship:
1. A per-org **ingest endpoint**: WSS ingest (Convex httpAction or thin relay) accepting Artisan's per-sample JSON, keyed by an org-scoped capture token.
2. A downloadable **`.aset` config** generated in-app ("Connect Artisan" wizard: pick machine → download settings → roast), pre-wired with the org's endpoint + per-sample `send()` of BT/ET (+ extras).
3. Backend: batched sample mutations (append to raw chunks + refresh the live downsampled array on the roast doc) → the existing reactive curve chart becomes **live** with zero chart work; roast auto-created on first samples after sentinel transition, closed on silence timeout.
4. Fits the funnel perfectly: "keep roasting the way you roast — Droptime adds the brain," now in real time.

**Risks:** interface drift across Artisan releases (pin + CI-test against new releases); requires Artisan running (acceptable — it's what these roasters already do).

### Tier 2 — Browser-native Web Serial capture (~4–6 weeks, after Tier 1 validates)
Direct in-app live roasting on desktop Chrome/Edge for **serial-class** devices: Kaleido (both serial generations, 9600/57600), TC4/Arduino bridges (incl. the Skywalker DIY bridge), and — pending a probe-the-actual-device spike — **Bullet R2** (CDC). Session resilience via permission persistence + reconnect events; **offline-first buffer in IndexedDB** with idempotent replay (roastId + sequence numbers) so a WiFi blip mid-first-crack loses nothing. Explicitly out of scope: Safari, mobile, Phidgets HID, Bullet R1.

### Tier 3 — Native bridge app (Tauri) — only if demand proves it
For Phidgets-HID and Bullet-R1 owners who refuse the Artisan path: the Cropster/RoastLog-style local bridge (libusb). Heavy (drivers, updater, three OSes), deferred until Tiers 1–2 usage data justifies it. Clean-room protocol work only (GPL).

### Cross-tier invariants
- Raw samples persisted verbatim; smoothing/RoR display-side (Artisan's proven model, already Droptime's).
- Capture is **read-only** in all tiers — machine *control* (heater/fan/drum, which Artisan does bidirectionally) is out of scope; the Skywalker 10 s watchdog and Bullet single-consumer constraint make control a liability we don't need yet.
- Live roast = same schema as imported roasts; the AI post-roast readout runs unchanged the moment DROP lands.

## 3. Open items for the spike week
1. Bullet R2 CDC: acquire/borrow a unit (or community tester) and confirm Web Serial can read it. **[med → needs proof]**
2. Artisan outbound: confirm event-annotation pushes (CHARGE/DROP outward) vs. samples-only; worst case, detect events server-side from the curve. **[med]**
3. MQTT (Artisan 4.2.0) as Tier-1.5: watch a release cycle before adopting.
4. Ingest transport: Convex httpAction per batch vs. a thin WSS relay — load-test at 1 s sampling × concurrent roasts.
