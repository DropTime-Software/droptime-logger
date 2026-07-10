# Phidgets linking spike — libphidget22 from BSD source, statically linked

**Date:** 2026-07-11. **Status: SUCCESS on macOS aarch64** — `cargo test --features phidgets`
links a source-built static libphidget22 and passes a no-hardware runtime-init test.
This documents exactly what worked, what the scoping doc feared vs. what is true,
the CI recipe per OS, and the recommended shape for the real driver work.

## Provenance

| What | Value |
|---|---|
| Source | `https://cdn.phidgets.com/downloads/phidget22/libraries/linux/libphidget22/libphidget22-1.25.20260512.tar.gz` |
| Version | **1.25.20260512** (pinned; newest at time of spike) |
| sha256 | `c2b2cf3f7da03a35ec7b074b5027bb95452a7e26f77a0ace38d7b2f667147d5f` |
| License | **BSD-3-Clause** (`COPYING` in the tarball, © 2015–2022 Phidgets Inc.) |
| Rust crates | `phidget` **=0.4.1** (MIT) over `phidget-sys` 0.1.5 (MIT, pre-generated bindings) |
| libusb (macOS) | Homebrew `libusb` (LGPL-2.1-or-later), `libusb-1.0.a` merged into our staged framework |

The "Linux" tarball is the **only** source distribution Phidgets publishes. The macOS
`.dmg` / `Phidget22_macosdevel_*.zip` and the Windows installers are **prebuilt binaries
under a proprietary EULA** — verified by unpacking `Phidget22_macosdevel_1.25.20260408.zip`
(contents: dylib, frameworks, xcframework, signed kext/dext — zero source). Never fetch or
redistribute those. When the driver ships, `THIRD-PARTY-NOTICES.md` needs the libphidget22
BSD attribution and the libusb LGPL notice (both AGPL-compatible; LGPL §4 satisfied because
our whole app is open source).

Crate-bindings vintage is the safe direction: `phidget-sys` 0.1.5's pre-generated bindings
are older than 1.25, so every symbol they reference exists in our newer lib.

## What works (the recipe)

```
./scripts/build-libphidget22.sh          # ~20 s on M4 Pro, incl. download
cargo test --features phidgets           # 95 + 1 tests green
```

The script stages into `target/phidget-prefix/` (gitignored via the root `target/` rule;
override with `DROPTIME_PHIDGET_PREFIX`):

- `include/phidget22.h`, `lib/libphidget22.a` — static, host arch, `--disable-shared`
- macOS only: `Frameworks/Phidget22.framework/Phidget22` — a **static framework** whose
  binary is `libphidget22.a` merged with Homebrew's `libusb-1.0.a` via `libtool -static`

`build.rs` (feature-gated on `CARGO_FEATURE_PHIDGETS`) adds the framework/native search
paths and the system libs. `tests/phidgets_link.rs` calls `Phidget_getLibraryVersion`
(returned `Phidget22 - Version 1.25 - Built Jul 11 2026`, number `1.25.20260512`) and runs a
`PhidgetManager` open → 100 ms → close cycle, proving symbol resolution **and** runtime init
(threads, libusb enumeration) with no hardware. `otool -L` on the test binary shows only
`libSystem` + `IOKit` + `CoreFoundation` + `Security` — libphidget22 and libusb are fully
embedded. **Static linking also dissolves the bundling fear**: no separate dylib to sign,
notarize, or rpath-fix; the Rust binary carries everything.

## Findings: feared vs. true

**1. ".framework vs dylib / PHIDGET_ROOT ignored on macOS" — half right, and the fix is
cheaper than feared.** `phidget-sys`'s build.rs honors `PHIDGET_ROOT` as a *search path* on
every OS (the `if let Ok` is not cfg-gated) — but on macOS the link directive is hard-coded
`cargo:rustc-link-lib=framework=Phidget22`, so no search path can ever make a plain
`.dylib`/`.a` match. The scoping doc's "repackage as framework" instinct was correct, with a
twist that removes all downsides: ld64 links a framework whose binary is a **static
archive** ("static framework"). We stage one; no fork, no `[patch.crates-io]`, no vendored
-sys crate. Shadowing hazard: `phidget-sys` also adds `-F /Library/Frameworks` (+
`/opt/homebrew/Frameworks` on arm, `/usr/local/Frameworks` on x64). Our `-F` is emitted by
the top-level crate and lands first on the link line (verified), but a dev machine with the
official Phidgets framework installed is a latent ambiguity — CI runners are clean.

**2. The wall nobody predicted: there is no macOS (or Windows) source support upstream.**
The tarball's `configure.ac` OS case accepts only `*linux*`/`freebsd*` and errors
`Unrecognized OS` otherwise, and it ships only `plat/linux`. The `_MACOSX`/IOKit personality
in the headers references files **not in the tarball** (`plat/windows/*` likewise). But the
tree is otherwise portable POSIX + libusb, so the Linux personality builds on Darwin with
three byte-sized patches (all applied by the script, each behind a grep anchor that fails
loudly on upstream drift):
  1. `configure`: route `*darwin*` through the linux case.
  2. `src/ext/mos/mos_byteorder.h`: `#if defined(_MACHINE) && _MACHINE == i386` compares two
     *undefined tokens* (0 == 0) on arm64, pulling x86 inline asm into every arch; regated on
     `__i386__ || __x86_64__`. (Latent upstream bug; gcc's lazy diagnostics hide it on Linux.)
  3. `sem_timedwait` doesn't exist on macOS; a `-include` prelude shims it with
     `sem_trywait` + 10 ms polling — mirroring upstream's own `#if defined(Darwin)` branch
     in `mos_lock-pthread.c`. No upstream file is edited for this.

**3. Cargo/rustc gotcha that cost an hour: rlib-metadata-carried link flags get dropped.**
`cargo:rustc-link-lib` from our build.rs rides *this crate's lib* metadata; an integration
test that only references `phidget` never links our lib, so those flags vanish and the link
fails on CoreFoundation symbols. Fix: emit system deps as `cargo:rustc-link-arg`, which goes
on the command line of every linkable target — and, bonus, lands *after* `-lphidget22`,
which GNU ld's left-to-right archive resolution requires on Linux anyway.

**4. Scoping-doc corrections.** `phidget` 0.4.1 exists now (2025-10-28, after the research);
we pin `=0.4.1` over `phidget-sys` 0.1.5. And `PHIDGET_ROOT` is not literally "ignored on
macOS" — it is honored but useless there (see finding 1).

## The one open risk — flag for the TMP1101 rig day

Enumeration works everywhere, but **macOS device *open* through the libusb backend is
unproven**. Phidgets' own macOS framework uses IOKit/HID and ships a DriverKit dext; our
build uses their Linux libusb path. If the VINT Hub (HUB0002) enumerates as a vendor-class
device, libusb should claim it cleanly. If it is HID-class and macOS's kernel HID driver
claims it, `libusb_detach_kernel_driver` on macOS requires root or a special entitlement
(that is why `Security.framework` is in the link: libusb's `darwin_detach_kernel_driver`
checks `SecTaskCopyValueForEntitlement`). **First action when hardware arrives:** plug the
hub into this Mac, run the manager test, and try a `TemperatureSensor::open_wait`. Fallbacks
if it fails, in order: (a) small IOKit-HID transport patch upstreamable to Phidgets (they
take patches; the `_MACOSX` code exists, just unshipped — ask them to include it in the
tarball); (b) Phidget Network Server loopback (documented, port 5001); (c) macOS users
install the official framework (user-installed ≠ redistributed) and we link dynamically
there. Do not build any of these speculatively.

## CI recipe (when the feature matrix lands)

Cache key for the staged prefix: `phidget-prefix-${{ runner.os }}-${{ runner.arch }}-1.25.20260512`,
path `apps/logger/src-tauri/target/phidget-prefix` (the script is stamp-file idempotent, so
cache misses just cost the ~1–2 min build).

- **macos-15 (arm64)** — proven locally on this Mac (same toolchain family):
  ```yaml
  - run: brew install libusb
  - run: ./scripts/build-libphidget22.sh
    working-directory: apps/logger/src-tauri
  - run: cargo test --features phidgets
    working-directory: apps/logger/src-tauri
  ```
- **macos-15-intel (x86_64)** — identical steps. `config.guess` detects the host,
  `brew --prefix libusb` resolves `/usr/local/...`, the byteorder patch keeps the (valid)
  x86 asm path, `MACOSX_DEPLOYMENT_TARGET=11.0` matches rustc's minimum. No arm-only logic
  anywhere in the script.
- **ubuntu-latest** — add `libusb-1.0-0-dev` to the existing apt line, then the same two
  steps. This is the library's home platform; the script applies no darwin patches there.
  Only the `.a` is staged, so `phidget-sys`'s `-lphidget22` resolves statically. Manager
  open on a device-less runner needs no udev rules (nothing is opened).
- **windows-latest** — **the script cannot help: the tarball has no `plat/windows` sources,
  so there is no from-source route at all** (this is a harder wall than macOS, where source
  builds; it is not a matter of MSYS2 effort). The coherent story instead: Windows users
  must run the Phidgets driver installer anyway for USB access, and it installs
  `phidget22.dll` + `phidget22.lib` at `C:\Program Files\Phidgets\Phidget22` — exactly
  `phidget-sys`'s default `PHIDGET_ROOT`. CI: silently install the official Phidgets
  installer (or unzip `Phidget22-windevel`), set `PHIDGET_ROOT`, run the same test. Using
  the EULA'd package in CI is *use*, not redistribution; we ship nothing of it. Long-term:
  ask Phidgets for Windows source or explicit redistribution permission (fire that email
  with the Kaleido one).

Build time observed: **~20–22 s total** on M4 Pro (download 2–3 s, configure ~8 s,
`make -j14` ~7 s, stage <1 s). Expect ~1–2 min on GitHub runners.

## Recommended shape for the real driver

Keep exactly this seam: `phidget` crate pinned (`=0.4.1`, vendor if it churns), `phidgets`
cargo feature stays off-by-default until the CI matrix above is green on mac + linux, then
becomes default-on for release builds per-platform. `PhidgetSource` per the scoping doc
(`capture/phidgets.rs`, trait-boundary mock as the primary test rig — this spike's link test
is the only piece that needs the real FFI). The staged-prefix pattern extends unchanged to
release/bundling because the link is static: `tauri build` output needs no extra files, no
dylib signing, no rpath work. Rig-day checklist, in order: HUB0002 attach event on macOS →
`open_wait` on a TMP1101 channel → TCC/Input-Monitoring behavior in the *signed* app →
then the same on Linux with the udev rule.
