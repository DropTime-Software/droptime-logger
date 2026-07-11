# Droptime Logger vs Artisan — Feature-Parity Audit

**Prepared:** July 10, 2026, against Artisan v4.2.0 (June 30, 2026). **Sources:** artisan-scope.org docs/blog/releases, GitHub metadata/issues/discussions, forums — **no Artisan source files were opened** (GPL firewall; this document is safe input for implementation work). Implementation caveat carried from the audit: docs describe *behavior*, not algorithms — reimplement smoothing/RoR/auto-detect from first principles against described behavior; never ask an agent to "match Artisan's algorithm exactly" (that pressure invites source contamination).

**The one-paragraph verdict:** Artisan's value concentrates in a small sacred core — live BT/ET + RoR curves, the CHARGE→DROP event flow with phases/DTR, background-profile replay, alarms/auto-detect, and reliable archiving — surrounded by an encyclopedic long tail most roasters never touch, and a machine-breadth moat (80+ brands) we do not contest at v0.1.0. We are already at or above parity on most of the sacred core; the audit exposes **three real gaps** (background replay UX, auto CHARGE/DROP for live devices, simple read-only alerts) which are now in the v0.1.0 scope. Our deliberate concessions are machine *control* (~40% of Artisan's value to its power users — positioning must own "scope + analytics, not a controller") and device breadth (beachhead strategy). Our "better" list is real and marketable.

---

## 1. The sacred core (parity bar — must not be worse)

| Artisan capability | Usage | Droptime Logger status | Verdict |
|---|---|---|---|
| Live BT/ET curves + delta/RoR curve + live numeric RoR ("the reason the tool is worth using") | core | Have: least-squares trailing RoR live, symmetric recompute post-roast, dropout/flatline rules, 60fps smoothed display | **Parity+** — smoother than 2s-tick rendering; must keep noise handling excellent on real probes (TC4 validation) |
| CHARGE→DRY→FC→FC END→DROP event flow → phases + DTR | core | Have: marker bar w/ huge targets, keyboard flow, same event vocabulary (muscle-memory compatible), auto turning point | **Parity** |
| Phase %/DTR visibility | core — **Artisan's #1 pure-UX complaint (issue #1200, open since 2023): the numbers vanish mid-roast** | BigNumbers + PhaseBars keep phase %, DTR, RoR permanently on screen | **Better — market this explicitly** |
| Background profile overlay + replay ("Playback Aid": upcoming-event cues) — THE consistency workflow | core | Chart supports ghost/target overlays; **no UX to roast against a prior roast, no event cues** | **GAP → added to v0.1.0 (§3.4a of the OSS plan)** |
| Auto CHARGE / auto DROP detection | core, commonly enabled; brittle in Artisan (alarm-config-dependent, discussion #1350) | Replay auto-charges; **no live-device detection** | **GAP → added to TC4 driver scope**; ship it robust-by-default, not config-dependent — a "better" |
| Alarms: popup/sound/TTS cues ("talking alarms" a documented favorite) | core-ish (read-only subset) | Only the planned projected-FC sound cue | **GAP (subset) → simple alerts added to v0.1.0**: BT/time-after-event triggers → notification/sound/speech. Machine-actuating alarm actions: conceded (control) |
| Reliable logging, archiving, upgrade-safe history | core — trust anchor | SQLite WAL + write-before-emit + crash recovery + `user_version` ladder | **Better** — Artisan has no mid-roast crash recovery story; ours is a headline |
| Autosave/batch filing | core | Auto-persisted DB (no files to manage), export on demand | Parity by construction; batch counter → v0.2 with library mgmt |
| Free + open source | the price anchor | v0.1.0 ships OSS (AGPL/Apache split) | **Parity** — deliberate |
| Machine breadth: 80+ brands, 50+ devices, MODBUS/S7/WebSocket/MQTT | THE moat | TC4 + replay at v0.1.0; Phidgets/MODBUS next per plan | **Conceded at launch** — honest README matrix; the wizard is the counter (below) |
| Data portability: .alog/CSV/Excel/JSON export; broad import | core | .alog + CSV import, .alog/CSV/JSON export in v0.1.0 scope | **Parity** on the paths that matter; note community ask #1575 (no clean .alog parser exists) — our Rust parser could be published as a crate: cheap goodwill |
| Time-to-first-roast on a *supported* machine | short — the good path | Wizard targets ≤3 min | Must match; the differentiation is the *unsupported/DIY* path (next section) |

## 2. Where we win (the marketable "better" list)

1. **Setup.** Artisan's documented Achilles heel: DIY/generic-rig setup is ports/baud/CH340-vs-FTDI/device-dialogs/close-the-other-app traps — one representative novice thread (GitHub #1074) ran **Jan 2023 → Nov 2024**. The wizard + hardware sniffer + driver-aware diagnostics is the single clearest differentiation. "Live temps in three minutes."
2. **The numbers never vanish.** Phase %/DTR/RoR/FC time stay on screen the whole roast (their #1200, unshipped for 3 years) — and the live curve gets maximum vertical real estate (the exact complaint pattern that hit RoasTime's July 2026 redesign).
3. **Zero data loss** — force-quit mid-roast recovers losslessly; samples hit disk before the screen. Artisan can't say that sentence.
4. **60fps smoothness** with honest data underneath.
5. **Modern packaging**: native Apple Silicon, signed/notarized installers (their ARM64 ask #584 and Flathub thread are years-old friction), ~15MB not a Python/Qt bundle.
6. **Config migration that works**: machine pins + one-file export — Artisan's own `.aset` restore is historically lossy (#149, #2029), so even Artisan-to-Artisan migration is untrusted. We do their config migration better than they do.
7. **The AI lane (cloud layer)**: Artisan v4.2 contains **no AI/ML anywhere** — no prediction, no anomaly detection, no assistant. Grounded readouts + Q&A remain unclaimed territory (Cropster's AI predicts curves; nobody explains roasts).
8. **A web/tablet path** (their recurring mobile ask): browser demo now, companion viewer next — the PyQt desktop can never follow.

## 3. Deliberate concessions (own them in positioning)

- **Machine control** — PID (hardware + software + 2-DOF/gain-scheduled), sliders→burner actuation, IO/serial/MODBUS/MQTT writes, Playback-Events auto-pilot. This is ~40% of Artisan's value to its power users and 100% out of scope by the read-only invariant (safety/liability + Bullet single-consumer + watchdog liabilities). **Positioning rule: we are the best roast scope + analytics, not a roast controller.** Never imply otherwise; the README says it plainly.
- **Device breadth** — beachhead (TC4 → Phidgets → MODBUS) with an honest matrix; the audit's warning is respected: don't chase 80 brands, win the setup experience on the brands we ship.
- **The encyclopedic tail** — Analyzer/polyfit, Transposer, Designer, flavor wheel, cup profiles, energy/CO2, symbolic formulas, quantifiers, Web-LCDs: documented-niche. Post-v1 candidates at most; the Comparator (multi-roast overlay + realign) is the one tail item with broad appeal — v0.2 candidate alongside history compare (we have ghost overlays; realign-at-event is cheap for us since we rebase at charge natively).
- **The business layer** — inventory/scheduling/traceability is artisan.plus (~€48/mo, proprietary, separate from the GPL app) — which **validates our open-core boundary and prices our anchor**: Droptime Cloud's free tier already beats artisan.plus's paid entry on the same jobs.

## 4. Scope changes applied to the v0.1.0 plan

1. **§3.4a Background replay ("Roast against a previous roast")** — pick any prior roast (or imported .alog) at setup → aligned-at-charge ghost on the live chart + upcoming-event cues (visual + optional sound/speech: "First crack on the reference in ~30s"). Read-only playback-aid only. ~1.5–2 days.
2. **TC4 driver scope** now includes **auto CHARGE/DROP detection** (first-principles temp-signature detection, on by default, one toggle — not alarm-config-dependent). +0.5–1 day.
3. **§3.5 polish** now includes **simple alerts**: user-set triggers (BT threshold / time after event) → notification, sound, or spoken cue (OS TTS via webview speechSynthesis). ~1 day.
4. Revised v0.1.0 estimate: **~3.5–4 focused weeks** (was ~3).
