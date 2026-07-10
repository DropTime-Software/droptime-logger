# Changelog

All notable changes to Droptime Logger are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

_Next up: Phidgets and MODBUS drivers, code signing + notarization, and a standalone driver SDK (Apache-2.0)._


## [0.1.0] - 2026-07-10

The first public release. Droptime Logger is a free, local-first, open-source roast logger — a roast **scope and analytics tool, not a controller**. It reads temperatures and never actuates hardware.

### Added

**Capture**

- TC4 / aArtisanQ-compatible serial driver — 115200 8N1, read-only (never sends OT1/OT2/IO3/PID). Covers TC4-emulating DIY builds and many Skywalker setups.
- Auto CHARGE/DROP detection from the temperature signature, on by default with a single toggle. Robust from first principles rather than dependent on alarm configuration.
- Replay / simulator source — log a complete roast with zero hardware attached.
- Hardware sniffer — enumerates serial ports, recognizes FTDI / CP210x / CH340 adapters by name, listens briefly to classify TC4 frame shape, and always offers a manual port/baud fallback. Includes a **Copy diagnostic** block that feeds the device-report issue template.
- Mid-roast failure banners for common serial problems (unplug/replug, port busy).

**Live roast**

- Live BT/ET curves plus delta/RoR, using a least-squares trailing rate-of-rise with a 60fps-smoothed display and dropout/flatline handling.
- Phase %, DTR, RoR, and first-crack time stay on screen for the entire roast (big numbers + phase bars).
- CHARGE → DRY → FC → FC END → DROP event flow with a fast keyboard marker path and automatic turning-point detection.
- Background replay — roast against any prior roast or imported `.alog`: an aligned-at-charge ghost curve on the live chart plus upcoming-event cues (visual chip and optional sound/speech). Playback aid only; it never drives the machine.
- Simple read-only alerts — user-set triggers (BT threshold, or time after an event) fire a notification, sound, or spoken cue.

**Data and storage**

- Local SQLite store (WAL) with write-before-emit durability and mid-roast crash recovery — force-quit and resume losslessly.
- Schema migration ladder tracked via `PRAGMA user_version`; the database is backed up before any migration.
- `.alog` import (batch, with count and preview before commit) — bring your Artisan history over in a drag-and-drop.
- Per-roast export to `.alog`, CSV, and JSON.
- Machines and coffees as managed, autocompleting lists; each machine carries its own source pin.
- Roast detail — compare-overlay against prior roasts (as ghosts), post-roast marker editing, editable coffee / weights / notes, and delete with confirmation.
- History screen with search and filter by coffee, and empty states that teach.

**Setup and app**

- First-run setup wizard (re-runnable from Settings): welcome with three paths — connect a roaster, try the demo, or import history — device scan, channel-role assignment with live preview, machine and °F/°C selection, an open-hardware probe guide, and history import.
- Native application menu (About, Check for updates, Keyboard shortcuts, Report an issue); ⌘, opens Settings.
- Keyboard-help overlay, window-state persistence, and macOS traffic-light inset handling.
- Update check against the GitHub Releases `latest.json` — **notice-only** in this release (no auto-update until signing lands).
- A "Droptime Cloud — coming soon" settings pane: an honest, visible seam for the optional paid layer (sync, AI readouts, team). Nothing in the free app depends on it.

**Packaging**

- Native installers: macOS `.dmg` (Apple Silicon and Intel), Windows `.exe` (NSIS), and an experimental Linux `AppImage`. Around 15 MB.

### Licensing

- App, console UI, capture engine, and `@droptime/roast-console` released under **AGPL-3.0-only**.
- Protocol documentation under `docs/protocols/` released under **Apache-2.0**.
- Contributions gated by a lightweight CLA and a clean-room provenance policy for drivers and file formats.

### Known limitations

- macOS builds are **not yet notarized** (right-click → Open on first launch) and Windows builds are **unsigned** (SmartScreen may warn). Signing lands in v0.1.1.
- No Phidgets or MODBUS drivers yet — both are on the v0.2 roadmap. Aillio Bullet R2 and Kaleido are under clean-room investigation. Device reports are welcome.
- The update check is notice-only; there is no automatic in-app update.
- Linux support is experimental and best-effort, not a supported tier.

[Unreleased]: https://github.com/OutsideTheBoxDev/droptime-logger/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/OutsideTheBoxDev/droptime-logger/releases/tag/v0.1.0
