# Droptime Logger — Open-Source v0.1.0 Release Plan

**Prepared:** July 10, 2026. **The decision this executes (Ryan):** the logger ships publicly as open source, v0.1.0 — a real MVP a stranger-roaster can download, set up through a wizard, and log an actual roast with, not a minimal demo. This is the tactical release plan; `droptime-logger.md` remains the strategic product plan and stays in force.

**Evidence base:** the OSS-precedents research pass (July 10). The honest headline: *community-written drivers are not a free lunch.* Artisan — 15 years old, the beloved GPL incumbent — has 57 contributors, is ~58% authored by one maintainer, and its 80-brand machine support is financed by **vendor sponsorship**, not community PRs. The projects that DO get mass community device support (Marlin: 1,315 contributors; Home Assistant: 21k/yr) had huge developer populations and an **out-of-tree plugin boundary**. What open-sourcing realistically buys us: **trust** (the anti-PE, anti-lock-in narrative against Cropster/Verdane — data sovereignty is the coffee community's stated religion), **a vendor-cooperation carrot** (Kaleido et al. sponsor Artisan *because* it's open), **a small enthusiast driver tail** (TC4/MODBUS DIY crowd), and **distribution** (r/roasting, Home-Barista, HN love an indie OSS story). Plan accordingly: architecture invites contributions, business doesn't depend on them.

---

## 1. The open-core boundary

**Open source (the public repo):** the entire desktop app — Rust capture engine, drivers, SQLite store, the React console (`roast-console`), simulator, fixtures. A crippled OSS app would poison the trust story; the app is genuinely complete for local-first solo roasting, forever.

**Commercial (stays in the monorepo):** Droptime Cloud — sync, AI readouts, the platform (inventory/wholesale/café). The validated pattern is exactly Artisan → artisan.plus and Home Assistant → Nabu Casa: the free open tool is whole; the paid layer is convenience + intelligence + team. The OSS app ships with a visible, honest seam: a "Droptime Cloud" settings pane ("Sync roasts, AI readouts, team — coming soon" in v0.1.0; live once the platform seam lands per droptime-logger.md Phase 2b).

**License (recommendation FINALIZED against the mechanics research, July 10):**
- **App + console UI + capture engine: AGPL-3.0.** The research settled the desktop question: AGPL's ordinary copyleft (not just the network clause) means a competitor who *incorporates* our code into a distributed app must open their entire app — and launching new under AGPL triggers no fork backlash (that comes from relicensing after ecosystem lock-in: Terraform/Elastic lesson; Grafana/Cal.com/Plausible launched or moved to AGPL with zero forks). AGPL is also the effective takedown lever against app-store clone grifters — the documented risk of publishing a polished console. And it keeps us OSI-approved "genuinely open source," which the coffee community (raised on GPL Artisan) will check.
- **Driver SDK + protocol docs: Apache-2.0** — the vendor/contributor touch-layer stays permissive (Home Assistant's deliberate choice); a driver library has no standalone consumer value to clone.
- **CLA required, not DCO-only — this is load-bearing.** `roast-console` is consumed by our commercial surfaces (droptime-web demo, droptime-app companion viewer), and drivers will ship inside future commercial apps. As copyright holder we can dual-license our own code freely — but external contributions accepted under AGPL+DCO alone could never ship in those proprietary surfaces (the Strapi/GitLab lesson: this decision is irreversible after the first external PR). Use a lightweight CLA-assistant flow so friction stays DCO-like.
- **Known loophole, accepted with mitigations:** GNU's "mere aggregation" doctrine means a competitor could ship our unmodified AGPL capture-core as a *separate sidecar binary* over arm's-length IPC and keep their console proprietary — and our own core/console split demonstrates the architecture. Mitigations, in the order the evidence says they actually work: development pace + the cloud/AI layer (the real moat), trademark (they can't call it Droptime — trademark policy in the repo), and AGPL still forcing source on any modification to the core itself. We do not contort the architecture to close this.

## 2. Repo topology & extraction

- **New public repo** (name/org = Ryan's call, e.g. `droptime/logger`). **Fresh git history** — the monorepo's history references unrelated products and must not leak.
- Moves out: `apps/droptime-logger` (app + src-tauri) and `packages/roast-console` (renamed `@droptime/roast-console`).
- The monorepo consumes `@droptime/roast-console` from npm (published by the public repo's release CI) for droptime-web's demo page and droptime-app's companion viewer. Until first publish, a git dependency bridges the gap.
- CONTRACTS.md ships in the public repo (it's the architecture doc contributors need); the sync-protocol section (§ outbox → Convex) moves to the monorepo — it describes the commercial seam.
- Public CI (GitHub Actions): `fmt + clippy -D warnings + cargo test` and `tsc + vitest + vite build` on every PR; `tauri-action` matrix builds (macOS aarch64/x64, Windows x64) on tag push producing Releases + `latest.json`.

## 3. MVP gaps — from "beautiful demo" to "a real logger" (the build list)

Ordered; each independently shippable. Estimates assume agentic dev, calibrated to how Phase 1 actually went.

### 3.1 The Setup Wizard (the headline gap) — ~3–4 days
First-run experience (and re-runnable from settings):
1. **Welcome** — what the logger is, local-first promise, three paths: *Connect a roaster* / *Try the demo* / *Import my history*.
2. **Device scan** — the hardware sniffer from droptime-logger.md §6: enumerate serial ports (VID/PID metadata first: FTDI/CP210x/CH340 recognized by name), and for candidates, listen ~3s and classify the frame shape (TC4 `ambient,ch1,ch2…` CSV signature). "Found a TC4-compatible device on `usbserial-1420` ✓" — with a manual port/baud picker fallback that never dead-ends.
3. **Channel roles** — live per-channel preview (numbers moving), user assigns BT/ET (+ ambient), saved as the machine's pin (`machines_local.source_pin`).
4. **Machine & units** — name the machine, °F/°C display.
5. **"Don't have a digital roaster?"** — the open-hardware path: recommended ~$35 Phidgets TMP1101 / TC4 boards with a visual probe-placement guide (marketing weapon per plan §6; content can be v0.1-thin, one good diagram).
6. **Import history** — drop-zone for `.alog`/CSV files (see 3.3), with count + preview before commit.
Demo path drops into today's replay flow unchanged.

> **Parity-audit scope additions (July 10, see `droptime-logger-artisan-parity.md`):** three Artisan-core gaps folded in below — background replay (3.4a), auto CHARGE/DROP in the TC4 driver, and simple read-only alerts in 3.5. Revised total: **~3.5–4 focused weeks**. Positioning rule from the audit: we are the best roast **scope + analytics**, not a roast **controller** — the README says it plainly.

### 3.2 The first real capture path: TC4/aArtisanQ serial driver — ~3.5–6 days
Includes **auto CHARGE/DROP detection** (first-principles temperature-signature detection, on by default, one toggle — robust rather than alarm-config-dependent, which is a documented Artisan fragility).
v0.1.0 must log a **real roast** or "logger" is false advertising. TC4 is the right first driver: protocol is BSD-3-Clause documented (`commands.txt` — cite it in PROVENANCE.md), 115200 8N1, `CHAN`/`UNITS`/`READ` → CSV; covers the whole TC4-emulating DIY/Skywalker ecosystem; testable without hardware via an ESP32/pyserial emulator (write the emulator as a repo dev-tool — it doubles as contributor test infra). Read-only (never send OT1/OT2/IO3/PID). Wire into: engine as `DeviceSource`, sniffer signature, wizard, mid-roast failure banners (unplug/replug/port-busy-by-Artisan — the §6 failure matrix, which is already specced).

### 3.3 .alog import/export — ~2 days
- **Import**: Rust parser via `py_literal` (schema from our documented key map + self-generated fixtures — never Artisan source; PROVENANCE.md), batch → local history. This is the switching weapon and the wizard's "Import my history" path.
- **Export**: `.alog` writer + CSV + JSON per roast (the data-sovereignty proof point; already promised in plan §5).

### 3.4a Background replay — "Roast against a previous roast" — ~1.5–2 days
THE Artisan consistency workflow (parity bar, not optional): pick any prior roast or imported .alog at setup → aligned-at-charge ghost curve on the live chart (the chart already supports ghosts; we rebase at charge natively so alignment is free) + upcoming-event cues ("First crack on the reference in ~30s" — visual chip + optional sound/speech). Playback-*aid* only; executing the reference's machine actions is control and stays out.

### 3.4 Library management + roast detail polish — ~2–3 days
- Machines & coffees as managed lists (autocomplete in per-roast setup; machines carry their source pin).
- Roast detail: **compare overlay** (pick prior roasts as ghosts — the chart already supports ghosts), edit markers post-roast (events history already stores raw taps), edit coffee/weights/notes, delete roast (confirm).
- History: search/filter by coffee, empty states that teach.

### 3.5 App-feel polish (the "very minimal" complaints) — ~2–3 days
- Native app menu (About, Check for updates, Keyboard shortcuts, Report an issue); ⌘, opens settings.
- Keyboard-help overlay (`?`), sound cues (optional tick at projected FC / drop approach), window-state persistence, proper macOS traffic-light inset handling.
- **Simple alerts** (the read-only-safe slice of Artisan's beloved alarm system): user-set triggers — BT threshold, time-after-event — firing a notification, sound, or spoken cue (webview speechSynthesis; "talking alarms" are a documented community favorite). Machine-actuating alarm actions stay out (control).
- Update check against GitHub Releases `latest.json`: full auto-update if signing is ready, else a "v0.2.0 available →" notice.
- Favicon/app-icon consistency pass (kills the dev-console 404 too).

### 3.6 Explicitly NOT blocking v0.1.0
Phidgets driver (needs the rig — v0.2 flagship), MODBUS (v0.2), Kaleido/Bullet (vendor/clean-room-gated), cloud sync + AI (commercial layer), companion viewer, Linux tier-1 (ship AppImage best-effort, label experimental).

## 4. OSS release infrastructure (the checklist)

- [ ] **README**: hero GIF (simulator roast at 10× — we have the harness), honest hardware matrix (TC4 ✓ / Phidgets soon / MODBUS soon / Kaleido+Bullet investigating), install instructions per OS, the local-first + export-anytime promise, Droptime Cloud one-liner.
- [ ] **LICENSE(s)** per §1 + `NOTICE` + THIRD-PARTY-NOTICES (libphidget22 comes v0.2; rusqlite/serialport/etc. now).
- [ ] **CONTRIBUTING.md** — the clean-room policy made public and enforceable, modeled on the ReactOS/Wine precedents the research surfaced: *driver PRs must document protocol provenance (vendor docs / own hardware captures / permissively-licensed references); porting, translating, or paraphrasing GPL code (Artisan or otherwise) is an automatic close; contributors who have studied Artisan's driver for machine X may not author Droptime's driver for machine X* (ReactOS-style module-level exposure rule); per-driver `PROVENANCE.md` required; **CLA via automated CLA-assistant** (see §1 — DCO alone forecloses commercial reuse of contributions) with the sanctioned-inputs list spelled out; PR checklist includes the provenance attestation.
- [ ] CODE_OF_CONDUCT (Contributor Covenant), SECURITY.md (private disclosure email).
- [ ] Issue templates: bug / **device report** (the community-tester funnel: rig, port, frame dump — the sniffer gets a "copy diagnostic" button feeding this) / feature.
- [ ] CI + release workflow per §2; branch protection; CHANGELOG (Keep-a-Changelog); tags `v0.1.0`.
- [ ] **Signing status decision**: macOS Developer ID + notarization if enrollment (plan Phase 0 item) lands in time — else v0.1.0 ships with the right-click-open caveat documented and signing becomes v0.1.1. Windows unsigned with SmartScreen note (Azure Artifact Signing when ready).
- [ ] GitHub Discussions on (Discord later if traction demands).

## 5. Launch motion

1. **droptime-web**: `/download` page + the in-browser demo page (`@droptime/roast-console` simulator — the plan's hero asset) + GitHub link in nav. Repo README links back.
2. **Posts**: r/roasting + Home-Barista build-in-public thread ("I built a modern, open-source roast logger — free forever, local-first, here's a 90-second roast"), demo GIF front and center; Hacker News Show HN when the TC4 driver has ≥1 external confirmation.
3. **Device-report campaign**: "own a Kaleido/Bullet/MODBUS machine? 10 minutes of your time gets your rig supported" → issue template funnel (this IS the plan's Phase-2.5 beta recruiting, now with an OSS carrot).
4. **Vendor outreach upgrade**: the Kaleido email (plan Phase 0) now offers Artisan-style cooperation with an open project — historically the thing coffee vendors actually sponsor.

## 6. Suggested build order (pick-up-and-go)

| # | Task | Size | Depends on |
|---|---|---|---|
| 1 | Repo extraction + rename + public CI green | 1–2d | repo name/org decision |
| 2 | .alog import/export (both sides) | 2d | — |
| 3 | Setup wizard (demo + import paths first) | 2d | 2 |
| 4 | TC4 driver + emulator dev-tool | 3–5d | — (parallel with 3) |
| 5 | Sniffer + wizard device path + failure matrix | 2d | 3, 4 |
| 6 | Library mgmt + roast detail polish | 2–3d | — |
| 7 | App-feel polish + update check | 2–3d | — |
| 8 | README/CONTRIBUTING/templates/licensing | 1–2d | license decision |
| 9 | Release pipeline + signing decision + v0.1.0 tag | 1–2d | 1, 8 |
| 10 | Launch (web pages, posts, device campaign) | 1–2d | 9 |

Realistic total: **~3.5–4 weeks of focused work** to a public v0.1.0 (incl. the parity-audit additions: background replay, auto charge/drop, simple alerts).

## 7. Open decisions (Ryan)

1. **Repo org + name** — `droptime/logger`? Standalone brand? (Recommendation: keep Droptime brand; the funnel is the point.)
2. **License final** — §1's recommendation is now evidence-backed (AGPL app/console/core + Apache driver SDK + CLA); your sign-off is what remains.
3. **DCO vs CLA** — resolved by the research: CLA, because roast-console and drivers flow into commercial surfaces and the decision is irreversible after the first external PR (Strapi/GitLab precedent). Keep friction low with cla-assistant automation.
4. **Ship v0.1.0 unsigned on macOS** or gate the release on Apple enrollment (~days). Recommendation: enroll now, sign v0.1.0 — first impressions with roasters happen once.
5. **Sync seam visibility** — show the "Droptime Cloud — coming soon" pane in v0.1.0 (honest funnel, my recommendation) or keep the OSS app clean of any commercial mention until sync works.
