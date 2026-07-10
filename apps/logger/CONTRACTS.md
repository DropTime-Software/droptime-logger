# Droptime Logger — Contracts

Phase 1 sections (§1–§4) are FROZEN; the v0.1.0 expansion (§5+) is FROZEN as of
July 10, 2026. Changes to this file require updating every consumer in the same
change. TS-side shared types live in `packages/roast-console/src/types.ts`
(the sibling contract). Canonical units: °F, seconds; `sessionSec` = seconds
since recording started; chart-time t = sessionSec − chargeSessionSec.

## 1. SQLite schema (v1 — `PRAGMA user_version = 1`)

DB file: `{app_local_data_dir}/droptime-logger.db` (macOS: `~/Library/Application Support/com.droptime.logger/`).
`journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000`, foreign_keys ON.
One writer thread owns all writes. Samples batched in ~1s transactions.
`wal_checkpoint(TRUNCATE)` after a roast finishes. Backup-copy the DB file before any
future migration; migrations are a strictly-ordered `user_version` ladder.

```sql
CREATE TABLE roasts (
  uuid TEXT PRIMARY KEY,              -- UUIDv7 minted at session start
  device_id TEXT NOT NULL,            -- per-install UUID (settings)
  source_id TEXT NOT NULL,            -- SourceInfo.id that captured it
  status TEXT NOT NULL,               -- recording | finished | abandoned
  started_wall_ms INTEGER NOT NULL,
  machine_local_id INTEGER,
  coffee_name TEXT,
  charge_weight_lb REAL,
  drop_weight_lb REAL,
  charge_session_sec REAL,            -- set by the charge event
  charge_temp_f REAL,
  turning_point_sec REAL, turning_point_temp_f REAL,
  dry_end_sec REAL, fc_start_sec REAL, fc_end_sec REAL,
  drop_sec REAL, drop_temp_f REAL,    -- all *_sec are seconds-from-charge
  notes TEXT,
  synced_batch_id TEXT                -- Convex roastBatches id after sync (Phase 2b)
);
CREATE TABLE samples (
  roast_uuid TEXT NOT NULL REFERENCES roasts(uuid),
  seq INTEGER NOT NULL,
  session_sec REAL NOT NULL,
  bt_f REAL NOT NULL,
  et_f REAL, ambient_f REAL, heater REAL, fan REAL, drum REAL,
  PRIMARY KEY (roast_uuid, seq)
) WITHOUT ROWID;                      -- append-only, never rewritten
CREATE TABLE events (
  roast_uuid TEXT NOT NULL REFERENCES roasts(uuid),
  kind TEXT NOT NULL,                 -- RoastEventKind (types.ts)
  session_sec REAL NOT NULL,
  note TEXT,
  created_ms INTEGER NOT NULL
);                                    -- raw tap history; roasts.* markers are canonical
CREATE TABLE machines_local (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  make TEXT,
  source_pin TEXT                     -- JSON: preferred source/port/channel-roles
);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);  -- device_id, units, etc.
CREATE TABLE outbox (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  roast_uuid TEXT,
  op TEXT NOT NULL,                   -- start_live | chunk | live_patch | finalize | import
  chunk_index INTEGER,
  payload_version INTEGER NOT NULL DEFAULT 1,
  payload BLOB NOT NULL,
  created_ms INTEGER NOT NULL,
  synced_ms INTEGER
);                                    -- reserved for the optional cloud-sync layer
CREATE INDEX idx_events_roast ON events(roast_uuid);
CREATE INDEX idx_outbox_pending ON outbox(synced_ms) WHERE synced_ms IS NULL;
```

## 2. IPC surface (Tauri commands; DTOs serialize camelCase via serde rename_all)

Stream type — `SampleEvent` (sent on `tauri::ipc::Channel<SampleEvent>`, ordered):
```ts
type SampleEvent =
  | { type: 'sample'; seq: number; sessionSec: number; btF: number; etF?: number;
      ambientF?: number; heater?: number; fan?: number; drum?: number }
  | { type: 'status'; kind: SourceStatusKind; message?: string; atSessionSec: number };
```

Commands:
- `ping() -> "pong"`
- `list_sources() -> SourceInfo[]` — Phase 1: the bundled replay fixtures (kind `replay`,
  id `replay:<fixtureName>`); device sources appear in Phase 2a.
- `start_session(args, onEvent: Channel<SampleEvent>) -> { roastUuid: string }`
  where `args = { sourceId: string; speed?: number; meta: { machineLocalId?: number;
  coffeeName?: string; chargeWeightLb?: number } }`. Spawns the capture engine;
  every sample is written to SQLite BEFORE it is emitted on the channel.
- `mark_event(args: { roastUuid: string; kind: RoastEventKind; sessionSec?: number;
  note?: string }) -> RoastMarkersDto` — appends to `events`, updates canonical
  markers on `roasts` (charge sets `charge_session_sec` + `charge_temp_f` from the
  nearest sample; drop sets `drop_sec`/`drop_temp_f`). `sessionSec` defaults to "now".
- `undo_event(args: { roastUuid: string; kind: RoastEventKind }) -> RoastMarkersDto`
- `finish_session(args: { roastUuid: string; dropWeightLb?: number; notes?: string })
   -> RoastSummaryDto` — stops capture, finalizes markers, checkpoints WAL,
  computes turning point if unmarked (math parity with roast-console/math).
- `abandon_session(args: { roastUuid: string }) -> void`
- `stop_capture(args: { roastUuid: string }) -> void` — stop streaming WITHOUT
  finalizing (the roast stays `recording` until finish/abandon). Used at the end
  of the post-drop cool-down window: capture deliberately continues 60
  roast-seconds past DROP (cool-down data kept; drop undo-able losslessly),
  visibly (summary chip), then the app calls this. finish/abandon are
  no-op-safe on an already-stopped source.
- `pending_recovery() -> RecoveryDto | null` — `roasts.status='recording'` from a
  previous process. `RecoveryDto = { roastUuid, startedWallMs, lastSeq,
  lastSessionSec, meta }`.
- `resume_session(args: { roastUuid: string }, onEvent: Channel<SampleEvent>)
   -> { resumedFromSeq: number }` — reattaches (replay source: continues fixture
  from last seq; device sources reconnect in Phase 2a). Emits a `status: reconnected`.
- `discard_recovery(args: { roastUuid: string }) -> void` — closes the orphan as
  `finished` at its last sample with a `gap` note (data preserved, per plan §5).
- `list_roasts() -> RoastSummaryDto[]` (newest first)
- `get_roast(args: { roastUuid: string }) -> { summary: RoastSummaryDto;
   samples: SampleDto[]; events: EventDto[] }`
- `get_setting(args: { key: string }) -> string | null` / `set_setting(args: { key, value })`

DTO field names mirror `types.ts` (RoastSummary, LiveSample, RoastEvent).
Errors: commands return `Result<T, LoggerError>` where `LoggerError` serializes to
`{ code: string; message: string }` — codes: `source_not_found`, `session_active`
(only one recording session at a time), `no_such_roast`, `db`, `io`.

## 3. Fixtures

`packages/roast-console/fixtures/*.json` in the `RoastFixture` shape (types.ts).
The Rust replay driver and the TS SimulatorSource replay the SAME files —
Rust embeds them via `include_str!` for Phase 1 (keeps list_sources dependency-free).
Baseline fixtures: `ethiopia-guji.json` (clean 11-min washed profile),
`colombia-stall.json` (post-dry-end stall → RoR crash — exercises anomaly UI),
`fast-decaf.json` (9-min decaf, steep RoR decline).

## 4. Webview source bridge (apps/droptime-logger/src/bridge/)

`TauriSampleSource implements SampleSource` (types.ts) wrapping the commands above.
Detection: `'__TAURI_INTERNALS__' in window` → Tauri mode; otherwise browser mode
(pure SimulatorSource, no persistence) so the app runs in a plain browser for dev/demo.

---

# v0.1.0 contract expansion (July 10, 2026 — FROZEN)

Everything below is additive; nothing in §1–§4 changes shape. Module ownership
is listed per section so parallel work stays disjoint; `ipc.rs`, `lib.rs`,
`model.rs`, `error.rs`, `schema.rs`, `dto.ts`, `ipc.ts`, `state/types.ts`,
`reducer.ts`, `App.tsx` and the mount-point stubs are integration-owned (edited
once, up front) — feature work fills the owned module files only.

## 5. SQLite migration v2

```sql
ALTER TABLE roasts ADD COLUMN coffee_local_id INTEGER;      -- optional link to coffees_local
ALTER TABLE roasts ADD COLUMN reference_roast_uuid TEXT;    -- background-replay reference used
ALTER TABLE roasts ADD COLUMN imported_from TEXT;           -- 'alog' | 'csv' when imported
ALTER TABLE machines_local ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
CREATE TABLE coffees_local (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  origin TEXT,
  process TEXT,
  notes TEXT,
  archived INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_roasts_started ON roasts(started_wall_ms DESC);
```

Imported roasts: `status='finished'`, `source_id='import:alog'` (or `import:csv`),
`imported_from` set, `device_id` = this install, `started_wall_ms` from the file when
recoverable else file mtime. Samples are stored rebased so that `session_sec = t + |min(t,0)|`
with `charge_session_sec` set accordingly (charge pins the rebase exactly like live capture).

`machines_local.source_pin` JSON (SourcePin, camelCase):
`{ "sourceId": "tc4:<portName>", "baud": 115200, "btChannel": 1, "etChannel": 2, "unit": "F" }`
— `btChannel`/`etChannel` are 1-based TC4 logical channels; `etChannel` optional.

## 6. New stream event (Channel<SampleEvent>)

```ts
| { type: 'marker'; kind: RoastEventKind; auto: boolean; atSessionSec: number;
    markers: RoastMarkersDto }
```

Emitted whenever the BACKEND changes canonical markers outside a `mark_event` call —
v0.1.0: auto CHARGE/DROP detection (`auto: true`). The UI applies it exactly like a
`mark_event` response and shows an undo affordance (`undo_event` works unchanged).

## 7. New IPC commands

All return `Result<T, LoggerError>`. New error codes: `not_implemented` (stub),
`port_busy`, `port_error`, `parse`, `invalid_args`, `preview_active`.

### 7.1 Import/export — module `src-tauri/src/alog/` + `store/imports.rs` (owner: importer)
- `preview_import(args: { paths: string[] }) -> ImportPreviewDto[]` — parse only, never
  writes. Per file: `{ path, ok, error?, coffeeName?, startedWallMs?, durationSec?,
  sampleCount?, markers?: RoastMarkersDto }`.
- `import_roasts(args: { paths: string[] }) -> { imported: number; roastUuids: string[];
  failed: Array<{ path: string; error: string }> }` — re-parses and commits.
- `export_roast(args: { roastUuid: string; format: 'alog'|'csv'|'json'; destPath: string })
   -> { path: string }` — destPath chosen by the frontend via the dialog plugin.
- Formats: `.alog` via Python-literal parse/write (`py_literal` crate; schema from our
  documented key map + self-generated fixtures ONLY — provenance rules in
  `src-tauri/src/alog/PROVENANCE.md`). CSV import: header-sniffed `time,bt[,et]`
  (comma/tab/semicolon); temperatures auto-detected °C vs °F by range and converted to °F.
  JSON export: `GetRoastResult` shape verbatim.

### 7.2 Serial + TC4 — modules `capture/serial.rs`, `capture/tc4.rs`, `capture/autodetect.rs` (owner: driver)
- `list_serial_ports() -> Array<{ portName: string; vid?: number; pid?: number;
  manufacturer?: string; product?: string; serialNumber?: string;
  chip?: 'ftdi'|'cp210x'|'ch340'|'other'; likely: boolean }>`
- `sniff_port(args: { portName: string; baud?: number /*115200*/; durationMs?: number /*3000*/ })
   -> { verdict: 'tc4'|'unknown'|'silent'; rawFrames: string[]; channels?: number[];
        diagnostic: string }` — `diagnostic` is the copy-pasteable device-report block
  (port, VID/PID, baud, frames) for the GitHub issue template. Port open failure →
  `port_busy`/`port_error`.
- `start_port_preview(args: { portName: string; baud?: number; unit?: 'F'|'C' },
   onEvent: Channel<PreviewEvent>) -> void` / `stop_port_preview() -> void` where
  `PreviewEvent = { channels: Array<number|null>; ambientF?: number; atMs: number }`
  (~1Hz, 1-based order as TC4 reports). One preview at a time (`preview_active`);
  refused while a session is active (`session_active`); `start_session` on the same
  port implicitly stops it.
- `start_session` source resolution: `sourceId` `replay:<fixture>` → ReplaySource (Phase 1),
  `tc4:<portName>` → Tc4Source. `StartSessionArgs` gains OPTIONAL `sourcePin?: SourcePin`
  (§5 shape; required for `tc4:`) — engine validates and rejects `invalid_args` when missing.
- TC4 protocol (READ-ONLY invariant: the driver NEVER writes OT1/OT2/IO3/PID/heater
  commands): 115200 8N1; `CHAN;1200` → `#OK`; `UNITS;F` → `#OK`; `READ` every 1s →
  `ambient,ch1,ch2,ch3,ch4[,heater,fan]` CSV. Non-numeric/short frames tolerated;
  3 consecutive read failures → `status: disconnected` + auto-reconnect loop w/ backoff
  (2s..30s) emitting `reconnected` on success (the §6 failure matrix).
- Auto CHARGE/DROP (`capture/autodetect.rs`): first-principles BT-signature detection fed
  from the engine sink for `device` sources only, per-kind enable via settings keys
  `autoMark.charge` / `autoMark.drop` ('on' default, 'off'); fires ≤1× per roast per kind;
  marks via store (same path as `mark_event`) then emits §6 `marker` event.

### 7.3 Library — module `store/library.rs` (owner: librarian)
- `list_machines() -> MachineDto[]` / `save_machine(args: { id?: number; name: string;
  make?: string; sourcePinJson?: string }) -> MachineDto` / `archive_machine(args: { id: number })`
  where `MachineDto = { id, name, make?, sourcePinJson?, archived }`. Machines are never
  hard-deleted (roasts reference them).
- `list_coffees() -> CoffeeDto[]` / `save_coffee(args: { id?: number; name: string;
  origin?: string; process?: string; notes?: string }) -> CoffeeDto` /
  `archive_coffee(args: { id: number })` with `CoffeeDto = { id, name, origin?, process?,
  notes?, archived }`.
- `update_roast(args: { roastUuid: string; patch: RoastPatchDto }) -> RoastSummaryDto` —
  patch fields: `coffeeName, coffeeLocalId, machineLocalId, chargeWeightLb, dropWeightLb,
  notes`; absent = unchanged, explicit `null` = clear (double-Option deserialization,
  helper in model.rs).
- `update_roast_markers(args: { roastUuid: string; markers: MarkerPatchDto })
   -> RoastMarkersDto` — editable: `turningPointSec, dryEndSec, fcStartSec, fcEndSec,
  dropSec` (absent/null semantics as above; charge is NOT editable in v0.1.0). Temps
  (`dropTempF`, `turningPointTempF`) recomputed from samples. Appends an `events` row
  (kind = edited marker, note `"edited"`) per change. Only on `finished` roasts.
- `delete_roast(args: { roastUuid: string }) -> void` — hard-deletes roast + samples +
  events + its outbox rows; `session_active` if it is the active session.

### 7.4 App shell — modules `menu.rs`, `update.rs` (owner: polisher)
- `get_app_info() -> { version: string; os: string; arch: string }`.
- `check_for_update() -> { currentVersion: string; latestVersion: string; url: string;
  isNewer: boolean } | null` — GETs the GitHub Releases `latest.json` (URL constant in
  `update.rs`; 3s timeout via `ureq`; errors → `Ok(None)`, never a user-facing failure).
  Notice-only in v0.1.0 (no auto-update until signing).
- Native menu (Rust `tauri::menu`, built in `menu.rs`): About, Check for Updates…,
  Settings… (⌘,), Keyboard Shortcuts (?), Report an Issue, standard Edit/Window items.
  Menu selections emit a webview event `menu://<id>` handled in `features/appShell`.

## 8. Frontend additions (owners in parentheses)

- `Screen` union += `'detail' | 'settings' | 'wizard'`; `NAVIGATE` gains optional
  `params?: { roastUuid?: string }` stored as `state.navParams` (integration).
- `SessionState` += `reference: ReferenceState | null` with action
  `{ type: 'SET_REFERENCE'; reference: ReferenceState | null }` where `ReferenceState =
  { roastUuid: string; label: string; curve: CurvePoint[]; markers: RoastMarkersSec }`
  (integration; consumed by reference feature).
- `SessionMeta`/`SessionMetaDto` += `referenceRoastUuid?: string`, `coffeeLocalId?: number`
  (persisted to roasts columns at start_session).
- Feature directories (each self-contained, mounted via pre-placed mount points):
  `features/import/` + `features/export/` (importer), `features/reference/` (reference:
  picker section on Setup, ghost + cue layer on Live; cue math lives in
  `packages/roast-console/src/replay/` as pure functions), `features/library/` (librarian:
  coffee/machine autocomplete fields on Setup, DetailScreen, History rework),
  `features/alerts/` + `features/appShell/` (polisher: alert rules engine + editor,
  keyboard-help overlay, sound cues, window-state persistence, menu event handling,
  update notice, Droptime Cloud pane in Settings).
- Alert rules — settings key `alerts.rules` (JSON array):
  `{ id: string; enabled: boolean; when: { type: 'btAtLeast'; btF: number }
     | { type: 'timeAfterEvent'; event: RoastEventKind; afterSec: number };
    action: { notify?: boolean; sound?: 'tick'|'chime'; speak?: string } }`
  — evaluated in the webview (≤1Hz derived tick), fire-once per rule per roast,
  READ-ONLY (never actuates hardware).
- Settings keys registry (all via get/set_setting): `units` ('F'|'C' display),
  `wizard.completed` ('true'), `autoMark.charge`/`autoMark.drop` ('on'|'off'),
  `alerts.rules` (JSON), `sound.cues` ('on'|'off'), `window.state` (JSON),
  `updates.lastCheckMs`, `updates.dismissed.<version>`.
- Tauri plugins (registered in lib.rs, integration): `dialog` (file open/save),
  `notification` (alert + auto-mark notices). Capabilities updated accordingly.

## 9. v0.1.0 non-goals (unchanged invariants)

No machine control of any kind (no OT1/OT2/IO3/PID writes — read-only is a hard
invariant, enforced in the TC4 driver by construction: no write path exists).
No cloud sync (outbox still written; flushed in Phase 2b). No charge-marker editing.
One session at a time. Store-before-emit stays sacred.
