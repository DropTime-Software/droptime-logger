<!--
  Copyright (c) 2026 Droptime / Ryan Luttrell
  SPDX-License-Identifier: Apache-2.0
  Licensed under the Apache License, Version 2.0 — see docs/protocols/LICENSE.
-->

# Generic MODBUS-TCP — Droptime notes

Droptime's working documentation of the Logger's generic MODBUS-TCP capture driver (`src-tauri/src/capture/modbus.rs` and the `modbus_emulator` example). If your roaster — or the PLC panel driving it — speaks MODBUS-TCP, this driver can log it: you tell the Logger *which registers* carry bean temperature and friends, and it polls them. There is no vendor-specific code path; the register map is entirely your configuration.

## Provenance

| # | Source | Type | License / basis | Accessed |
|---|--------|------|-----------------|----------|
| 1 | MODBUS Application Protocol Specification V1.1b3, [modbus.org](https://modbus.org/specs.php) | Open, freely published protocol standard | Publicly published specification | July 2026 |
| 2 | `tokio-modbus` crate (framing, MBAP transport, client/server plumbing) | Permissive open-source library | MIT OR Apache-2.0 | July 2026 |
| 3 | The repo's self-authored `modbus_emulator` rig and fixtures | Self-generated | Ours | July 2026 |

**Declarations:** MODBUS is an open standard, so protocol knowledge here is unrestricted. This repository embeds **no manufacturer's register map** — register addresses are user configuration, sourced by each owner from their own machine's documentation. No GPL implementation of a MODBUS roaster integration (Artisan's or any other) was read, ported, translated, or paraphrased in producing this document or the driver, and proprietary register maps must never be transcribed into this repository from one.

## Scope: read-only, always

MODBUS has write function codes (05/06/0F/10/16/17…) that can actuate whatever the registers control — heaters, fans, drum drives. **Droptime Logger never sends any of them.** The driver can only issue **FC03 (Read Holding Registers)** and **FC04 (Read Input Registers)**: its only wire-facing method takes the function code from a closed two-variant enum, both variants reads, and no write path exists in the code by construction. Contributions adding one are rejected. The bundled emulator answers any non-read request with `IllegalFunction` and a loud warning, and the test suite audits every request the driver ever transmits.

## Transport

- **MODBUS-TCP** (MBAP framing over TCP), standard port **502** — some panels use another port; the source id carries it either way.
- Source id: `modbus-tcp:<host>:<port>` (e.g. `modbus-tcp:192.168.1.199:502`). Hostnames, IPv4, and bracketed IPv6 all work.
- **Unit id** (a.k.a. slave id): default `1`. Most TCP devices ignore it or expect 1 or 255; gateways bridging to RTU need the real slave address. Check your manual.
- MODBUS-RTU (serial RS-485) is **not** covered by this driver yet; it is on the roadmap.

## Register addressing — read this before typing numbers

The single most common MODBUS configuration mistake is the addressing convention. The Logger uses **raw protocol data addresses**: the 0-based number that actually goes on the wire, 0–65535.

Manuals write addresses in one of three styles:

| Your manual says | Convention | What to enter as `register` |
|------------------|------------|------------------------------|
| `40001`, `40018`, … | Modicon 4xxxx (holding) | subtract 40001 → `0`, `17`, … with `"kind": "holding"` |
| `30001`, `30005`, … | Modicon 3xxxx (input) | subtract 30001 → `0`, `4`, … with `"kind": "input"` |
| `0`, `17`, `4001`, … ("address", "offset", "register number") | raw protocol address | use as-is; the manual should say which table (holding vs input) |

Some vendors additionally document 1-based "register numbers" without the table prefix — if everything reads one register off, try the neighbouring address. The live values make it obvious: a bean-temperature register should sit near ambient before charge and in the roast range during one.

## The SourcePin (what the Logger stores for a MODBUS machine)

The machine's saved pin — and the `sourcePin` passed to `start_session` — is this JSON document:

```json
{
  "sourceId": "modbus-tcp:192.168.1.199:502",
  "unitId": 1,
  "channels": [
    { "role": "bt", "register": 0, "kind": "input", "dataType": "u16", "scale": 0.1, "offset": 0 },
    { "role": "et", "register": 1, "kind": "input", "dataType": "u16", "scale": 0.1 }
  ],
  "unit": "C",
  "pollMs": 1000
}
```

| Field | Required | Default | Meaning |
|-------|----------|---------|---------|
| `sourceId` | yes | — | `modbus-tcp:<host>:<port>` |
| `unitId` | no | `1` | MODBUS unit/slave identifier, 0–255 |
| `channels` | yes | — | One entry per value to log; a `bt` role is required |
| `channels[].role` | yes | — | `bt` \| `et` \| `ambient` \| `heater` \| `fan` \| `drum` — each at most once |
| `channels[].register` | yes | — | Raw protocol data address, 0–65535 (see the addressing table) |
| `channels[].kind` | no | `holding` | `holding` (FC03) \| `input` (FC04) |
| `channels[].dataType` | no | `u16` | `u16` \| `i16` \| `f32` \| `f32-swapped` |
| `channels[].scale` | no | `1.0` | `value = decode(registers) × scale + offset` |
| `channels[].offset` | no | `0.0` | see above |
| `unit` | no | `F` | Unit the **temperature registers carry**: `F` or `C`. `bt`/`et`/`ambient` are converted to °F internally; `heater`/`fan`/`drum` pass through untouched |
| `pollMs` | no | `1000` | Poll cadence in milliseconds (floor 50) |

### Data types and word order

- `u16` / `i16` — one register, unsigned / two's-complement signed. Machines that publish tenths (e.g. `2137` = 213.7 °C) want `scale: 0.1`.
- `f32` — two consecutive registers holding an IEEE-754 float, **standard word order** (high word at the lower address).
- `f32-swapped` — the same float with the **words swapped** (low word at the lower address). Vendors are split on this; if your f32 temperature decodes as something absurd (±10³⁰, tiny denormals, NaN), switch word order.

Batching: channels on contiguous registers of the same table are read in a single request (up to the spec's 125-register limit); anything else is read individually. You don't configure this — it just keeps the per-second traffic minimal.

## Polling, timing, and failure behavior

- Poll cadence: every `pollMs` (default **1 s**) the driver reads the whole channel map and emits one sample.
- **3 consecutive poll failures** (timeouts, transport errors, MODBUS exception responses, or a missing/non-finite BT value) → the driver reports `disconnected` and enters a reconnect loop with backoff from **2 s to 30 s**, reporting `reconnected` once a probe poll returns a usable BT again. Network blips mid-roast are expected, recoverable events; the sample sequence stays gap-free across them.
- A NaN/Infinity float (some devices publish NaN for a disconnected probe) is treated as a failed read for `bt`, and as "absent" for optional roles — garbage never reaches the roast log.
- Exception responses like `IllegalDataAddress` almost always mean a wrong register address or the wrong table (`holding` vs `input`).

## Finding your machine's register map

The Logger deliberately ships **no vendor register maps** — a slightly-wrong builtin map is worse than your machine's documentation. Where to look:

- **Your machine's manual / manufacturer support.** Search the manual for "MODBUS", "register", or "third-party integration". If the manual doesn't cover it, ask the manufacturer — many provide a register list to owners on request.
- **Loring** roasters expose a MODBUS-TCP mode for third-party logging software that the owner enables on the machine's panel; Loring provides the connection details and register documentation to owners — check your manual or Loring support. Enter the registers they document using the addressing table above.
- **Probat** machines with the networked data option can expose process values over the plant network; the register documentation comes with that option. Ask Probat which option your machine has.
- **PLC-retrofitted drums** (custom panels, Click/Siemens/Allen-Bradley builds): whoever programmed the panel chose the register layout — ask them, or read the PLC program's MODBUS mapping table.
- **Giesen is not MODBUS.** Giesen profile systems speak Siemens S7 protocols; this driver does not support them, and no MODBUS configuration will. Don't burn an afternoon trying.

When in doubt, point the channel at your best guess and watch the live value: BT should sit near room/ambient temperature on a cold machine and track the roast when one is running.

## The emulator (no-hardware rig)

Contributors and the test suite use an in-process MODBUS-TCP server instead of a roaster:

```sh
cd apps/logger/src-tauri
cargo run --example modbus_emulator                # built-in synthetic roast, port 5020
cargo run --example modbus_emulator -- \
    --fixture ../../../packages/roast-console/fixtures/ethiopia-guji.json \
    --speed 10 --port 1502                         # replay a fixture at 10×
```

Default register map (any unit id accepted):

| table | address | value |
|-------|---------|-------|
| input | 0 | BT × 10 (u16 — pin: `dataType "u16"`, `scale 0.1`) |
| input | 1 | ET × 10 (u16) |
| input | 2–3 | BT as `f32`, standard word order |
| input | 4–5 | ET as `f32`, standard word order |
| input | 6–7 | BT as `f32`, **swapped** word order |
| input | 8 | heater duty % × 10 (constant 45.0) |
| input | 9 | fan duty % × 10 (constant 60.0) |
| holding | 0 | BT × 10 (u16) |
| holding | 1 | ET × 10 (u16) |

Everything else answers `IllegalDataAddress`; any non-FC03/FC04 request answers `IllegalFunction` and prints a warning — this rig is also the read-only invariant's watchdog.

## Troubleshooting quick table

| Symptom | Likely cause |
|---------|--------------|
| `port_error: connection refused` | MODBUS-TCP interface not enabled on the machine, wrong port, or a firewall |
| `invalid_args` on start | Pin has no `channels`, no `bt` role, a duplicate role, or a malformed field |
| Immediate `disconnected`, no samples | Wrong register address or table — the device answers with exceptions |
| Values exactly 10× too big/small | Missing or wrong `scale` |
| Temperatures ~half/double expected | `unit` says `F` but the registers carry `C` (or vice versa) |
| f32 reads absurd magnitudes or NaN | Word order — switch `f32` ↔ `f32-swapped` |
| Everything shifted by one register | 0-based vs 1-based addressing — try the neighbouring address |
