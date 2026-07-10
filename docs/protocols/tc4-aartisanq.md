<!--
  Copyright (c) 2026 Droptime / Ryan Luttrell
  SPDX-License-Identifier: Apache-2.0
  Licensed under the Apache License, Version 2.0 — see docs/protocols/LICENSE.
-->

# TC4 / aArtisanQ serial protocol — Droptime notes

Droptime's working documentation of the TC4/aArtisanQ serial protocol as the Logger speaks it. This file describes **documented and observed device behavior** — it is a sanctioned input for implementation work under the clean-room policy (see CONTRIBUTING.md), and it is what `src-tauri/src/capture/tc4.rs` and the `tc4_emulator` example are built from.

## Provenance

| # | Source | Type | License / basis | Accessed |
|---|--------|------|-----------------|----------|
| 1 | aArtisanQ protocol documentation (`commands.txt`) and firmware configuration headers, MLG Properties LLC / Jim Gallt — [greencardigan/TC4-shield](https://github.com/greencardigan/TC4-shield) | Vendor/permissive protocol docs | BSD-3-Clause | July 2026 |
| 2 | Our own serial captures from TC4-compatible rigs (sniffer diagnostic blocks) and the repo's self-authored `tc4_emulator` fixtures | Own captures / self-generated | Ours | July 2026 |
| 3 | Public behavior documentation: TC4/aArtisan user guides and community forum descriptions of device behavior (descriptions only, never code) | Public behavior docs | Fair reference | July 2026 |

**Declaration:** no GPL implementation of this protocol (Artisan's or any other) was read, ported, translated, or paraphrased in producing this document or the driver it specifies.

## Scope: read-only, always

The aArtisanQ command set includes control verbs — `OT1`, `OT2`, `IO3`, `PID`, `DCFAN`, and friends — that drive heaters, fans, and PID loops. **Droptime Logger never sends any of them.** The Logger is a roast scope and analytics tool, not a roast controller; the driver contains no write path for control commands by construction, and contributions adding one are rejected. The only commands the Logger transmits are the four configuration/poll commands documented below, all of which are read-only with respect to the machine.

## Physical layer

- **Transport:** USB serial (a USB-to-UART bridge on the board), **115200 baud, 8N1, no flow control** — the aArtisanQ default.
- **Legacy baud rates:** older aArtisan-family builds were compiled at 19200 or 57600. The v0.1.0 sniffer auto-detects 115200 only; the setup wizard's manual baud picker covers legacy builds.
- **Boot delay:** most TC4 rigs are Arduino-based and **reset when the port opens** (the DTR toggle). Allow ~2 seconds after opening the port before sending the first command; expect boot chatter (`#`-prefixed lines) during that window.

## Framing

Newline-delimited ASCII in both directions.

- **Commands** are a verb plus semicolon-separated arguments, terminated by `\n`:
  `NAME;arg1;arg2;...`
- **Responses** come in two kinds:
  - **Data lines** — bare CSV of decimal numbers (the `READ` response).
  - **Non-data lines** — anything prefixed with `#`: acknowledgments, boot banners, comments, errors. The exact ack text varies by firmware build (`#OK`, `# Active channels set to 1200`, …).

**Parser rule:** treat *any* line starting with `#` as a non-data ack/comment; treat only well-formed numeric CSV lines as data; tolerate and discard short, empty, or non-numeric lines. Accept `\n` or `\r\n` terminators.

## Commands the Logger sends

| Command | Example | Response | Meaning |
|---------|---------|----------|---------|
| `CHAN` | `CHAN;1200` | `#`-ack | Map physical thermocouple inputs to logical channels. Four digits, one per physical input TC1–TC4 in order; each digit's value is the logical channel that input reports as, `0` disables it. `CHAN;1200` = TC1 → logical 1, TC2 → logical 2, TC3/TC4 off. |
| `UNITS` | `UNITS;F` | `#`-ack | Temperature unit for all reported values, including ambient: `F` or `C`. |
| `FILT` | `FILT;70;70;70;70` | `#`-ack | Optional. Per-logical-channel firmware filtering level, percent 0–100 (higher = heavier smoothing of reported values). Read-only-safe: it shapes reporting, it does not actuate anything. The Logger leaves firmware defaults unless the user configures otherwise. |
| `READ` | `READ` | one CSV data line | Poll for the current sample. See below. |

Session sequence: open port → wait out the boot delay → `CHAN;1200` → `UNITS;F` → (`FILT` if configured) → `READ` once per second.

## `READ` response format and field-count variants

```
ambient,chan1,chan2[,chan3,chan4][,heater,fan]
```

Example (two active channels, °F): `72.50,367.25,341.00,0.00,0.00`

- **Field 1 — ambient:** the board's cold-junction/ambient sensor, in the selected unit.
- **Then one field per logical channel,** in ascending logical order as configured by `CHAN`. Inactive trailing channels may be reported as `0.00` or omitted entirely, depending on the firmware build.
- **Optional trailing fields:** aArtisanQ builds with power reporting append **heater duty %** and **fan duty %**.

Field counts seen in the wild, all valid:

| Fields | Shape | Typical firmware |
|--------|-------|------------------|
| 3 | `ambient,ch1,ch2` | aArtisan, 2 active channels |
| 5 | `ambient,ch1,ch2,ch3,ch4` | aArtisan/aArtisanQ, 4 channels |
| 7 | `ambient,ch1,ch2,ch3,ch4,heater,fan` | aArtisanQ with power reporting |

**Parse positionally and defensively:** field 1 is always ambient; consume up to four channel fields; if exactly two more fields remain, they are heater and fan. Never index a fixed layout. Values are decimals (typically one or two decimal places). The Logger maps logical channels to roles via the machine's saved pin (`btChannel`/`etChannel`, 1-based), chosen by the user against a live preview in the setup wizard.

## Polling, timing, and failure behavior

- Poll cadence: `READ` at **1 Hz**. Replies normally arrive well within the poll interval.
- **3 consecutive read failures** (timeouts or unparseable responses) → the driver reports `disconnected` and enters a reconnect loop with backoff from **2 s to 30 s**, reporting `reconnected` on success. Unplug/replug mid-roast is an expected, recoverable event.
- A port that opens but stays silent usually means a wrong baud rate, a board still booting, or a rig that only speaks when polled — always send `READ` before concluding anything.
- The single most common failure for migrating users: **the port is already held by another running app** (e.g. Artisan). Serial ports are exclusive; close the other app.

## Identifying TC4 rigs — common USB-serial bridge chips

TC4 boards and TC4-emulating rigs (ESP32 builds, Skywalker mods, clones) reach the OS through a handful of USB-to-UART bridges. VID/PID identifies the **bridge chip, not the protocol** — plenty of non-roaster devices share these IDs — so the Logger uses this table only to rank *likely* candidates, then confirms by frame shape (below).

| Bridge chip | VID | PID | Notes |
|-------------|-----|-----|-------|
| FTDI FT232R | `0x0403` | `0x6001` | Classic Arduino + TC4 shield rigs |
| FTDI FT-X (FT231X et al.) | `0x0403` | `0x6015` | Newer FTDI-based boards |
| Silicon Labs CP210x | `0x10C4` | `0xEA60` | Very common on ESP32 dev boards |
| WCH CH340/CH341 | `0x1A86` | `0x7523` | Ubiquitous on clones |
| WCH CH9102 | `0x1A86` | `0x55D4` | Newer ESP32 dev boards |
| Arduino Uno R3 (ATmega16U2) | `0x2341` | `0x0043` | Genuine Uno as the TC4 host |
| Arduino Mega 2560 | `0x2341` | `0x0042` | Genuine Mega as the TC4 host |
| Espressif native USB | `0x303A` | various | ESP32-S2/S3/C3 with on-chip USB-CDC |

## Sniffer classification

The setup wizard's hardware sniffer confirms a TC4 rig by behavior, not by VID/PID:

1. Open the candidate port at 115200, wait out the boot delay.
2. Send `READ` (read-only) and listen ~3 seconds.
3. **Verdict `tc4`:** a line of 3–7 comma-separated decimals whose first field is a plausible ambient temperature.
4. **Verdict `silent`:** the port opened but nothing numeric came back (wrong baud, not a TC4, or still booting).
5. **Verdict `unknown`:** traffic arrived but didn't match the signature — worth a [device report](https://github.com/RyanLuttrell/droptime-logger/issues/new?template=device-report.yml); the sniffer's diagnostic block (port, VID/PID, baud, raw frames) is built to be pasted into one.
