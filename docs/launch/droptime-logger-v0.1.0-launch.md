# Droptime Logger v0.1.0 — Launch Kit

Prepared July 10, 2026. Everything here is a DRAFT for Ryan's edit pass. Post order:
repo public → LinkedIn + r/roasting same morning → Home-Barista thread that evening →
Show HN only after ≥1 external TC4 confirmation lands (per the release plan).

---

## 1. LinkedIn post (Ryan's voice)

I open-sourced a coffee roasting tool today.

For the last while I've been building Droptime, software for coffee roasters. Along the
way I kept hearing the same two things from people who roast for a living:

1. The tools they trust most are the ones they can't lose — no subscription that walks
away with their roast history, no cloud outage in the middle of a roast.
2. Setting up roast logging is still, in 2026, an afternoon of serial ports, baud rates,
and driver forums.

So we built Droptime Logger and released it as open source (AGPL). It's a free desktop
app for Mac and Windows that logs your roasts live from your roasting machine:

— Local-first: every sample is written to disk on your machine before it even hits the
screen. Force-quit mid-roast and you lose nothing.
— A setup wizard that finds your roaster: plug in, scan, name your channels. Live
temperatures in about three minutes.
— The numbers roasters actually watch — rate of rise, phase percentages, development
time ratio — stay on screen the whole roast.
— Roast against a previous roast: your reference curve as a ghost on the live chart,
with a heads-up before each milestone ("first crack on the reference in ~30s").
— Import your entire Artisan history in one drag-and-drop. Export everything, anytime
(.alog, CSV, JSON). Your data is yours — that's the whole point.

It's a scope, not a controller: it reads temperatures and never actuates your machine.

v0.1.0 supports TC4/aArtisanQ-compatible rigs (most DIY and many Skywalker setups),
with Phidgets and MODBUS machines next. If you roast on something we don't support yet,
ten minutes of your time and a diagnostic paste gets your rig on the roadmap.

Free forever. No account. ~15 MB.

Download + source: https://github.com/DropTime-Software/droptime-logger

If you know a roaster, send this to them. And if you build software for a craft
community — the trust you earn by giving the core tool away and keeping people's data
theirs is worth more than any lock-in.

*(suggested: attach the demo GIF or a 30–60s screen recording of a roast at 10× speed)*

---

## 2. r/roasting post

**Title:** I built a free, open-source roast logger with a setup wizard that actually
finds your roaster (Mac/Windows)

**Body:**

After too many evenings fighting serial ports, I built the roast logger I wanted:

* **Free + open source (AGPL), local-first.** Everything lives in a local database on
  your machine. Samples are written to disk *before* they're drawn — force-quit
  mid-roast, reopen, and the roast resumes where it died.
* **Setup wizard with a port sniffer.** It scans your serial ports, recognizes
  TC4/aArtisanQ-style frames, shows you live per-channel numbers, and you tap which one
  is BT/ET. No baud-rate guessing.
* **RoR, phases, DTR always on screen.** Nothing vanishes mid-roast.
* **Roast against a previous roast** — ghost curve aligned at charge + a cue before each
  reference milestone (optional spoken alerts).
* **Auto CHARGE/DROP detection** on by default (you can toggle it off).
* **Artisan import/export.** Drag your .alog history in; export any roast back out as
  .alog/CSV/JSON. Not trying to trap anyone.

Honest limits at v0.1.0: TC4-compatible rigs only for live capture (Phidgets + MODBUS
are next), it's a scope not a controller (it will never drive your burner), and the
installers are unsigned for another week or two (right-click → Open on macOS).

It's a demo-in-two-clicks app too — there's a built-in replay mode with real roast
profiles if you just want to poke at it with zero hardware.

If your machine isn't supported: there's a "device report" flow — the sniffer generates
a diagnostic block you paste into a GitHub issue, and that's genuinely how the roadmap
gets ordered.

GitHub (code + downloads): https://github.com/DropTime-Software/droptime-logger

Would love brutal feedback, especially from TC4/Skywalker folks who can confirm capture
on real hardware.

**Posting notes:** flair as "Tool/Software" if available; answer every comment for the
first 48h; the "confirm capture on real hardware" ask is the Show HN gate — track
confirmations in the thread.

---

## 3. Home-Barista thread notes (build-in-public angle)

Longer-form than Reddit; H-B rewards engineering detail and respect for prior art.
Beats to hit:

1. Open with respect for Artisan — 15 years, the tool that taught everyone what roast
   logging is. This is not an Artisan attack; it's a different set of trade-offs
   (setup UX + data durability + analytics vs. breadth + machine control).
2. The engineering story people enjoy: samples hit SQLite (WAL) before the UI sees
   them; crash recovery resumes mid-roast; the replay/simulator architecture means
   every feature is testable without hardware.
3. The clean-room point, stated plainly: no Artisan source was read; the TC4 protocol
   came from the BSD-licensed aArtisanQ command docs; the .alog format from documented
   behavior + self-generated fixtures. (H-B has Artisan contributors — this will be
   checked. It's true, say it confidently.)
4. The ask: device reports + TC4 hardware confirmations; contributors welcome, CLA +
   provenance rules explained in CONTRIBUTING.
5. Disclose the commercial relationship honestly if asked (and preemptively in the
   first post, one line): the logger is free forever and complete; Droptime the company
   plans a paid cloud layer (sync/AI/team) later — that's how the free tool stays
   funded. The coffee community respects this exact model (Artisan → artisan.plus).

## 4. Show HN (LATER — gated on ≥1 external hardware confirmation)

**Title:** Show HN: Droptime Logger – open-source, local-first roast logging for coffee

**URL:** https://github.com/DropTime-Software/droptime-logger

**First comment (post immediately after submitting):**

Hi HN — I run a small software company that builds software for coffee roasters.
This started as an itch about how fragile roast-logging setups are and turned into a
free, open-source desktop app (Tauri: Rust core + React front end) for Mac and
Windows.

The engineering decisions people here might find interesting:

*Durability first.* A roast is a 12-minute, unrepeatable physical event — you
can't re-run it because your logger crashed. So every sample is written to
SQLite (WAL mode) before the UI is allowed to render it. Force-quit the app
mid-roast, reopen it, and it reattaches to the recording and continues from the
last persisted sample. The store-before-emit rule is enforced in the capture
engine, not the UI.

*Hardware without hardware.* The serial driver (TC4/aArtisanQ protocol,
read-only by construction — the command enum simply has no write variants) is
developed and tested against a pty-based emulator that replays real roast
fixtures: `cargo run --example tc4_emulator`. CI runs a full end-to-end roast —
capture, auto charge/drop detection, unplug/reconnect — with zero physical
hardware. The emulator doubles as the contributor onramp: you can write and
test a driver without owning the machine.

*Clean-room, on purpose.* Artisan (the 15-year GPL incumbent) is excellent and
we didn't want its code anywhere near ours: the protocol came from the
BSD-licensed aArtisanQ command docs, the .alog import/export from documented
behavior and self-generated fixtures, and CONTRIBUTING has a ReactOS-style
module-exposure rule for driver authors. AGPL for the app, Apache-2.0 for the
protocol docs, CLA for contributions.

*The business model, stated plainly:* the logger is complete and free forever —
local data, export everything (.alog/CSV/JSON), no account. The company plans a
paid cloud layer (sync, AI roast analysis, team features) later. Artisan →
artisan.plus and Home Assistant → Nabu Casa are the pattern.

v0.1.0 supports TC4-compatible rigs; Phidgets and MODBUS (Loring/Giesen-class
machines) are next. If you roast on something else, the built-in port sniffer
generates a diagnostic block you can paste into a device-report issue — that's
genuinely how we're prioritizing drivers.

Happy to answer anything about the Rust capture engine, Tauri, the clean-room
process, or coffee.

**Timing notes:** Tuesday–Thursday, 8–10am ET. Don't submit until at least one
stranger has confirmed live capture on real TC4 hardware (the r/roasting thread
is the recruiting ground) — HN will ask "does it actually work?" and the answer
must be "yes, users confirm." No vote-begging; just mention "we're on HN today"
in the existing Reddit/H-B threads.

## 5. Asset checklist

- [ ] demo.gif — simulator roast at 10×, ~20s loop, light theme (goes in the repo's
      docs/media/ + the README hero + LinkedIn)
- [ ] macos-open.png — right-click→Open screenshot for the README install section
- [ ] 60s screen recording (LinkedIn native video performs better than a link)
- [ ] Repo social-preview image (Settings → Social preview; 1280×640)
