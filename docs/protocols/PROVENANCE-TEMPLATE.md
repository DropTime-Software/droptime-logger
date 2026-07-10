<!--
  Copyright (c) 2026 Droptime / Ryan Luttrell
  SPDX-License-Identifier: Apache-2.0
  Licensed under the Apache License, Version 2.0 — see docs/protocols/LICENSE.
-->

# PROVENANCE — `<driver or format name>`

<!--
  HOW TO USE THIS TEMPLATE

  Every driver or file-format PR must include a PROVENANCE.md, placed in the
  module directory it covers (e.g. src-tauri/src/capture/<driver>/PROVENANCE.md)
  and kept up to date by every later PR that touches that module.

  Copy this file, fill in every section, and delete the instruction comments.
  "See CONTRIBUTING.md" is not an answer to any field — be specific. If a
  section truly doesn't apply (e.g. no captures were used), say so explicitly
  rather than deleting the section.

  The point of this file: every protocol fact in the module must trace to a
  sanctioned input (the list in CONTRIBUTING.md, "The clean-room policy").
  If you can't source a fact, you can't ship it.
-->

## Protocol

- **Name / machine family:** <!-- e.g. "TC4 / aArtisanQ serial", "Phidgets TMP1101" -->
- **Transport:** <!-- e.g. USB serial 115200 8N1 / USB CDC-ACM / MODBUS-TCP -->
- **Droptime modules covered:** <!-- repo-relative paths, e.g. src-tauri/src/capture/tc4.rs -->

## Sources

<!--
  List EVERY source a protocol fact in this module derives from. Each source
  must be one of the sanctioned input types from CONTRIBUTING.md:
  vendor docs · BSD-3-Clause aArtisanQ commands.txt · own serial captures ·
  our documented key maps / self-generated fixtures · public behavior
  documentation (manuals, forum descriptions of behavior — never code).
  Link to the exact document/revision where possible, and record the date you
  accessed it — public pages change.
-->

| # | Source | Type (sanctioned input) | License / basis for use | Link | Date accessed |
|---|--------|-------------------------|-------------------------|------|---------------|
| 1 |        |                         |                         |      |               |
| 2 |        |                         |                         |      |               |

## Author exposure declaration

<!--
  Answer for every code author on the PR (reviewers and testers are exempt).
  The module-level exposure rule: someone who has studied a GPL driver
  implementation for machine X may not author Droptime's driver for machine X.
-->

- I / we have **not** read, ported, translated, or paraphrased any GPL implementation of this protocol (Artisan or otherwise): **yes / no**
- I / we have **not** studied GPL driver source for this machine family in a way that would violate the module-level exposure rule: **yes / no**
- If any answer above needs qualification, explain here (e.g. two-person spec/implementer separation was used — name who specified and who implemented):

## Capture methodology

<!--
  Fill in if any facts derive from your own hardware captures. If none were
  used, write "No captures used — sources above are documentation only."
-->

- **Hardware:** <!-- machine, board, probes, USB bridge chip -->
- **Firmware / device software version:** <!-- as exact as you can get -->
- **How captured:** <!-- e.g. Droptime sniffer diagnostic block, serial tap, logic analyzer, USB capture of your own device's traffic -->
- **Fixture files generated and where they live in the repo:** <!-- paths -->

## Sign-off

<!-- Name/handle and date. Later PRs touching this module append a row. -->

| Author | Date | Change |
|--------|------|--------|
|        |      | Initial provenance record |
