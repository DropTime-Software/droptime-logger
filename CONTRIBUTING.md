# Contributing to Droptime Logger

Thanks for being here. Droptime Logger is built by people who care about roasting coffee well and about software that respects the people using it. Contributions of every size are welcome — from a typo fix to a new driver — and a few kinds of help matter more than you might think:

- **Device reports — the single most valuable contribution.** Own a machine we don't support? Ten minutes of your time gets your rig on the roadmap. Open a [device report](https://github.com/OutsideTheBoxDev/droptime-logger/issues/new?template=device-report.yml) — the in-app hardware sniffer has a "copy diagnostic" button that produces a block you can paste straight into the template.
- **Drivers** for new machines and probes (read the [clean-room policy](#the-clean-room-policy) first — it is strict, and it is how we keep this project shippable).
- **Fixtures** — interesting roast profiles, weird serial frames, edge-case `.alog` files you generated from your own history. Real-world data makes the math honest.
- **Docs** — setup guides, probe-placement notes, protocol documentation in `docs/protocols/`.
- **Translations** — not wired up yet. If you want to help localize the Logger, open an issue so we know which languages to prioritize when the i18n scaffolding lands.

Bug reports are always welcome too. If something crashed mid-roast, tell us — recovering losslessly from exactly that is a core promise, and we treat regressions there as top severity.

This project follows our [Code of Conduct](CODE_OF_CONDUCT.md). Security issues go through [SECURITY.md](SECURITY.md), not the public tracker.

## Development setup

Prerequisites: **Node ≥ 20**, **pnpm ≥ 9**, **Rust ≥ 1.82** (stable), and the [Tauri 2 system dependencies](https://tauri.app/start/prerequisites/) for your OS (on Ubuntu: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libudev-dev`).

```sh
pnpm install
pnpm dev     # launches the desktop app (tauri dev)
pnpm test    # the full suite: vitest + cargo test
```

For fast Rust-only iteration, work from the engine crate directly:

```sh
cd apps/logger/src-tauri
cargo test
```

The app also runs in a plain browser (no Tauri, no persistence, simulator only) — it detects the missing Tauri bridge and falls back to the TypeScript simulator automatically, which is handy for pure UI work.

### No hardware? No problem.

Two rigs let you develop everything without a roaster:

1. **The built-in replay simulator.** Launch the app and take the "Try the demo" path — it replays real roast fixtures through the entire engine, storage and all.
2. **The TC4 emulator example** — the no-hardware dev rig for driver work:

   ```sh
   cd apps/logger/src-tauri
   cargo run --example tc4_emulator
   ```

   It opens a virtual serial port (a pty) speaking the aArtisanQ protocol and prints the port path. Point the setup wizard's manual port picker at that path and you are "connected to a roaster." This is the rig the TC4 driver itself was developed against, and it is what CI exercises. Pty-based, so macOS and Linux; on Windows, use the replay simulator.

## Architecture in ten lines

1. The **Rust capture engine** (`apps/logger/src-tauri`) owns devices, sessions, and a local **SQLite** database (WAL mode, one writer thread).
2. **Store-before-emit is sacred:** every sample is written to SQLite *before* it is emitted to the UI. Force-quit mid-roast and recovery is lossless. Never reorder this.
3. The **React console** (`packages/roast-console`, published as `@droptime/roast-console`) renders curves, markers, phases, and math — as pure components fed by a `SampleSource`.
4. The app shell (`apps/logger/src`) bridges the two over Tauri IPC; in a plain browser it falls back to the TypeScript simulator.
5. Drivers implement `DeviceSource` in `src-tauri/src/capture/`; the replay driver and the TS simulator replay the **same fixture files**, so tests agree across languages.
6. **READ-ONLY hardware invariant:** the Logger is a roast scope and analytics tool, not a roast controller. Drivers never actuate hardware — no PID, no burner, no fan writes. No write path exists, by construction. Do not add one.
7. Roast math (RoR, phases, turning point) lives in `packages/roast-console/src/math` with a Rust twin in `src-tauri/src/math.rs` — the two are kept in parity.
8. IPC commands, DTOs, the SQLite schema, and error codes are **frozen contracts**: read [`apps/logger/CONTRACTS.md`](apps/logger/CONTRACTS.md) before touching any of them.
9. Canonical units internally: °F and seconds; display units are a setting.
10. One recording session at a time; events are raw tap history, `roasts.*` markers are canonical.

## Code style

- **Rust:** `cargo fmt` formatted, and clippy-clean at `-D warnings` — CI runs `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` on every PR. Match the existing test style: unit tests in-module under `#[cfg(test)]`, integration behavior tested through the store/engine seams.
- **TypeScript:** strict mode, vitest tests colocated with the code they cover. Follow the existing feature-directory pattern (`features/<name>/` self-contained, mounted at pre-placed mount points).
- **Dependencies:** be reluctant. A ~15 MB installer is a feature. New crates or npm packages need a reason in the PR description.

## The clean-room policy

This is the section to read twice. Droptime Logger lives alongside Artisan — the beloved, 15-year-old GPL roasting app — and we keep an absolute licensing firewall between the two codebases. This is not about Artisan being off-limits as *software*; it's a great project we respect and recommend. It is about keeping our code's provenance clean enough that every line here is provably ours to license.

The rules, which are enforced on every driver and file-format PR:

1. **Provenance is required.** Every driver or file-format PR must include (or update) a per-driver `PROVENANCE.md` documenting where every protocol fact came from: vendor documentation, your own hardware captures, or permissively-licensed references only. Use the template at [`docs/protocols/PROVENANCE-TEMPLATE.md`](docs/protocols/PROVENANCE-TEMPLATE.md). A driver PR without one will not be reviewed.
2. **Porting GPL code is an automatic close.** Porting, translating, or paraphrasing GPL code — Artisan's or anyone else's — gets the PR closed automatically. No exceptions, no matter how small the snippet or how different the language.
3. **The module-level exposure rule.** If you have studied Artisan's driver source for machine X, you may not author Droptime's driver for machine X. You can still help enormously on that machine: file device reports, contribute serial captures from your own hardware, test builds, and document observed behavior — someone unexposed writes the code.
4. **Sanctioned inputs** — the complete list of what a driver may be built from:
   - vendor protocol documentation;
   - the BSD-3-Clause aArtisanQ `commands.txt`;
   - your own serial captures (the sniffer's diagnostic blocks are exactly this);
   - our documented key maps and self-generated fixtures;
   - public *behavior* documentation — manuals, forum posts describing what a device does (not code).

   If your source isn't on this list, ask in an issue before you write code.
5. **Attestation.** The PR checklist includes a provenance attestation checkbox. Checking it is a statement of fact, and we take it at your word — which is why violations are handled bluntly.

When in doubt, describe *behavior*, never algorithms from protected code, and reimplement from first principles. It is slower. It is also the only way this project stays distributable.

## Licensing and the CLA

The repo license is **AGPL-3.0-only** (root [`LICENSE`](LICENSE)) covering the app, the console UI, and the capture engine. Protocol documentation under `docs/protocols/` is **Apache-2.0** (see the LICENSE file in that directory) so vendors and contributors can use it freely. When the standalone driver SDK crate splits out (v0.2, alongside the Phidgets driver), it will ship **Apache-2.0** as well.

**All contributions require signing our CLA.** Here is why, plainly: contributions to this repo may be incorporated into Droptime's commercial products under other licenses. The CLA grants Droptime a perpetual, irrevocable, worldwide, royalty-free right to relicense your contribution, plus a patent grant. You **retain your copyright**, and you take on no obligations beyond that grant. Full text: [`CLA.md`](CLA.md).

Signing is one comment: when you open your first PR, a bot will ask you to reply

> I have read the CLA Document and I hereby sign the CLA

on the PR. That's it — it covers all your future contributions, and the check turns green automatically.

## Pull requests

For anything bigger than a small fix, open an issue first so we can agree on direction before you invest the time. Keep PRs focused — one change per PR reviews fast.

Before you open one:

- [ ] `pnpm test` passes (vitest + cargo test)
- [ ] `cargo fmt --check` passes and `cargo clippy --all-targets -- -D warnings` is clean
- [ ] Driver or file-format change? `PROVENANCE.md` is included/updated and the provenance attestation box is checked
- [ ] CLA signed (the bot will prompt you on your first PR)

## Releases and versioning

We follow [semver](https://semver.org). While the app is at 0.x, minor versions may include breaking changes; the SQLite schema only ever migrates forward (a strictly-ordered `user_version` ladder — see CONTRACTS.md). Every user-visible change gets a line in [`CHANGELOG.md`](CHANGELOG.md) (Keep a Changelog format) in the PR that makes it. Maintainers cut releases by tagging `v*`, which builds and publishes the installers.

Questions? GitHub Discussions is open — come say hi, and tell us what you roast on.
