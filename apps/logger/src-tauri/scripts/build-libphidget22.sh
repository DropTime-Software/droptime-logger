#!/usr/bin/env bash
# build-libphidget22.sh — fetch, patch, and build libphidget22 as a STATIC
# library, staged into a predictable prefix for the `phidgets` cargo feature.
#
# PROVENANCE
#   Source:  https://cdn.phidgets.com/downloads/phidget22/libraries/linux/libphidget22/
#   License: BSD-3-Clause (COPYING in the tarball; Copyright Phidgets Inc.)
#            This is the ONLY source distribution Phidgets publishes. The
#            macOS .dmg / macosdevel .zip and Windows installers are PREBUILT
#            binaries under a proprietary EULA — never fetch or bundle those.
#   The pinned tarball below is what we compile; the sha256 was computed from
#   a fresh download on 2026-07-11 and must never change without bumping the
#   version alongside it.
#
# WHAT THIS PRODUCES (under $PREFIX, default: <crate>/target/phidget-prefix)
#   include/phidget22.h                     public header
#   lib/libphidget22.a                      static lib (host arch)
#   Frameworks/Phidget22.framework/         macOS only: a minimal STATIC
#     Phidget22                             framework whose binary is
#                                           libphidget22.a merged with
#                                           libusb-1.0.a (see WHY below)
#
# WHY THE FRAMEWORK (macOS): phidget-sys's build.rs hard-codes
# `cargo:rustc-link-lib=framework=Phidget22` on macOS — a plain dylib/.a is
# never matched, no matter what PHIDGET_ROOT says. Instead of patching the
# crate we stage our static archive AS a framework: ld64 happily links a
# framework whose binary is a static archive ("static framework"). Merging
# libusb into it makes the framework self-contained and link-order-proof.
#
# PATCHES APPLIED (kept byte-minimal; documented in capture/PHIDGETS-SPIKE.md)
#   1. configure:        accept *darwin* in the OS case (upstream supports
#                        linux/freebsd only; the tree itself is POSIX+libusb
#                        and compiles fine on Darwin as the Linux personality)
#   2. mos_byteorder.h:  `#if defined(_MACHINE) && _MACHINE == i386` compares
#                        two undefined tokens (0 == 0) on non-x86, pulling x86
#                        inline asm onto arm64; gate on __i386__/__x86_64__
#   3. darwin-shim.h:    macOS has no sem_timedwait(); injected via -include,
#                        emulates it with sem_trywait + 10 ms polling (same
#                        approach as upstream's own `#if defined(Darwin)`
#                        branch in mos_lock-pthread.c)
#
# USAGE
#   ./scripts/build-libphidget22.sh            # build for the host
#   DROPTIME_PHIDGET_PREFIX=/some/dir ./scripts/build-libphidget22.sh
#
# Requirements: curl, make, a C compiler; macOS: brew + `brew install libusb`;
# Linux: libusb-1.0-0-dev (apt) / libusb1-devel (dnf).
# Windows is NOT handled by this script — see capture/PHIDGETS-SPIKE.md.

set -euo pipefail

PHIDGET22_VERSION="1.25.20260512"
PHIDGET22_SHA256="c2b2cf3f7da03a35ec7b074b5027bb95452a7e26f77a0ace38d7b2f667147d5f"
PHIDGET22_URL="https://cdn.phidgets.com/downloads/phidget22/libraries/linux/libphidget22/libphidget22-${PHIDGET22_VERSION}.tar.gz"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE_DIR="$(dirname "$SCRIPT_DIR")"
PREFIX="${DROPTIME_PHIDGET_PREFIX:-$CRATE_DIR/target/phidget-prefix}"
BUILD_DIR="$PREFIX/build"
SRC_DIR="$BUILD_DIR/libphidget22-$PHIDGET22_VERSION"
TARBALL="$BUILD_DIR/libphidget22-$PHIDGET22_VERSION.tar.gz"
OS="$(uname -s)"
START_TS=$(date +%s)

say() { printf '\033[1m[build-libphidget22]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[build-libphidget22] ERROR:\033[0m %s\n' "$*" >&2; exit 1; }

# Idempotency: skip everything if the staged lib is already this version.
STAMP="$PREFIX/.libphidget22-$PHIDGET22_VERSION-$(uname -m).done"
if [[ -f "$STAMP" && -f "$PREFIX/lib/libphidget22.a" ]]; then
  say "already staged at $PREFIX (stamp: $(basename "$STAMP")) — nothing to do"
  exit 0
fi

mkdir -p "$BUILD_DIR"

# --- 1. Fetch + verify -------------------------------------------------------
if [[ ! -f "$TARBALL" ]]; then
  say "fetching libphidget22 $PHIDGET22_VERSION (BSD-3-Clause source tarball)"
  curl -fsSL -o "$TARBALL.tmp" "$PHIDGET22_URL"
  mv "$TARBALL.tmp" "$TARBALL"
fi

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL="$(sha256sum "$TARBALL" | awk '{print $1}')"
else
  ACTUAL="$(shasum -a 256 "$TARBALL" | awk '{print $1}')"
fi
[[ "$ACTUAL" == "$PHIDGET22_SHA256" ]] \
  || die "sha256 mismatch for $TARBALL: expected $PHIDGET22_SHA256, got $ACTUAL"
say "sha256 verified: $ACTUAL"

rm -rf "$SRC_DIR"
tar -xzf "$TARBALL" -C "$BUILD_DIR"
[[ -d "$SRC_DIR" ]] || die "tarball did not extract to $SRC_DIR"

# --- 2. Patch ----------------------------------------------------------------
cd "$SRC_DIR"

# Patch 2 (all platforms): fix the undefined-token comparison that selects the
# x86 inline-asm endian header on every architecture.
grep -q '#if defined(_MACHINE) && _MACHINE == i386' src/ext/mos/mos_byteorder.h \
  || die "mos_byteorder.h patch anchor missing — upstream changed; re-audit patches"
sed -e 's/#if defined(_MACHINE) \&\& _MACHINE == i386/#if defined(__i386__) || defined(__x86_64__)/' \
  src/ext/mos/mos_byteorder.h > src/ext/mos/mos_byteorder.h.patched
mv src/ext/mos/mos_byteorder.h.patched src/ext/mos/mos_byteorder.h

EXTRA_CPPFLAGS=""
EXTRA_LDFLAGS=""
if [[ "$OS" == "Darwin" ]]; then
  # Patch 1: let configure route Darwin through the Linux (POSIX + libusb) case.
  grep -q '^\*linux\*)$' configure \
    || die "configure patch anchor missing — upstream changed; re-audit patches"
  sed -e 's/^\*linux\*)$/*linux* | *darwin*)/' configure > configure.patched
  mv configure.patched configure
  chmod +x configure

  # Patch 3: sem_timedwait shim, injected without touching upstream sources.
  cat > darwin-shim.h <<'EOF'
/* darwin-shim.h — minimal POSIX shims so libphidget22's Linux personality
 * compiles on Darwin. Injected with `-include`; no source files modified. */
#ifndef DROPTIME_DARWIN_SHIM_H
#define DROPTIME_DARWIN_SHIM_H
#ifdef __APPLE__

#include <errno.h>
#include <semaphore.h>
#include <time.h>
#include <unistd.h>

/* macOS has no sem_timedwait(); emulate with trywait + 10 ms polling,
 * mirroring the polling loop libphidget22 itself uses in its own
 * `#if defined(Darwin)` branch of mos_namedlock_timedlock(). */
static inline int droptime_sem_timedwait(sem_t *sem, const struct timespec *abs_timeout) {
	for (;;) {
		if (sem_trywait(sem) == 0)
			return 0;
		if (errno != EAGAIN && errno != EINTR)
			return -1;
		struct timespec now;
		clock_gettime(CLOCK_REALTIME, &now);
		if (now.tv_sec > abs_timeout->tv_sec ||
		    (now.tv_sec == abs_timeout->tv_sec && now.tv_nsec >= abs_timeout->tv_nsec)) {
			errno = ETIMEDOUT;
			return -1;
		}
		usleep(10 * 1000);
	}
}
#define sem_timedwait droptime_sem_timedwait

#endif /* __APPLE__ */
#endif /* DROPTIME_DARWIN_SHIM_H */
EOF

  command -v brew >/dev/null 2>&1 \
    || die "Homebrew required on macOS to locate libusb (brew install libusb)"
  LIBUSB_PREFIX="$(brew --prefix libusb 2>/dev/null)" \
    && [[ -f "$LIBUSB_PREFIX/lib/libusb-1.0.a" ]] \
    || die "libusb static lib not found — run: brew install libusb"
  EXTRA_CPPFLAGS="-I$LIBUSB_PREFIX/include -include $SRC_DIR/darwin-shim.h"
  EXTRA_LDFLAGS="-L$LIBUSB_PREFIX/lib"
fi

# --- 3. Configure + build (static only) ---------------------------------------
if [[ "$OS" == "Darwin" ]]; then
  # Match rustc's aarch64-apple-darwin minimum so ld does not warn that every
  # object was "built for newer macOS version than being linked".
  export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
fi
say "configuring (static, prefix: $PREFIX)"
./configure --prefix="$PREFIX" --enable-static --disable-shared \
  CPPFLAGS="$EXTRA_CPPFLAGS" LDFLAGS="$EXTRA_LDFLAGS" \
  > "$BUILD_DIR/configure.log" 2>&1 \
  || die "configure failed — see $BUILD_DIR/configure.log"

NPROC="$( (command -v nproc >/dev/null && nproc) || sysctl -n hw.ncpu )"
say "building with -j$NPROC"
make -j"$NPROC" > "$BUILD_DIR/make.log" 2>&1 \
  || die "make failed — see $BUILD_DIR/make.log"
make install > "$BUILD_DIR/install.log" 2>&1 \
  || die "make install failed — see $BUILD_DIR/install.log"
[[ -f "$PREFIX/lib/libphidget22.a" ]] || die "libphidget22.a missing after install"

# --- 4. macOS: stage the static framework phidget-sys insists on --------------
if [[ "$OS" == "Darwin" ]]; then
  FW="$PREFIX/Frameworks/Phidget22.framework"
  say "staging static Phidget22.framework (libphidget22.a + libusb-1.0.a merged)"
  rm -rf "$FW"
  mkdir -p "$FW"
  # Apple libtool merges the archives; the result is a self-contained static
  # framework binary, so the Rust link needs no -lusb-1.0 and no -L ordering.
  libtool -static -o "$FW/Phidget22" \
    "$PREFIX/lib/libphidget22.a" "$LIBUSB_PREFIX/lib/libusb-1.0.a" \
    2> >(grep -v "has no symbols" >&2 || true)
  [[ -f "$FW/Phidget22" ]] || die "framework staging failed"
fi

touch "$STAMP"
ELAPSED=$(( $(date +%s) - START_TS ))
say "done in ${ELAPSED}s — staged at $PREFIX"
say "next: cargo test --features phidgets   (from $CRATE_DIR)"
