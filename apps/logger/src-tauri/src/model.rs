//! Shared DTOs — the Rust mirror of `packages/roast-console/src/types.ts`.
//!
//! Field names serialize camelCase (CONTRACTS.md §2); `SampleEvent` is tagged
//! with `type`. Canonical units everywhere: °F, seconds. `sessionSec` = seconds
//! since recording start; chart t = sessionSec − chargeSessionSec.

use serde::{Deserialize, Deserializer, Serialize};

/// Deserialize helper for PATCH semantics (CONTRACTS.md §7.3): field absent →
/// `None` (leave unchanged); explicit `null` → `Some(None)` (clear); value →
/// `Some(Some(v))`. Use with `#[serde(default, deserialize_with = "double_option")]`.
pub fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}

// ---------------------------------------------------------------------------
// Stream events (tauri::ipc::Channel<SampleEvent>)
// ---------------------------------------------------------------------------

/// Mirrors `SourceStatusKind` in types.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStatusKind {
    Connected,
    Disconnected,
    Reconnected,
    Flatline,
    Gap,
    Ended,
}

/// The ordered stream sent on `Channel<SampleEvent>`; mirrors the contract's
/// `SampleEvent` union (`{ type: 'sample' | 'status', ... }`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SampleEvent {
    #[serde(rename_all = "camelCase")]
    Sample {
        seq: u64,
        session_sec: f64,
        bt_f: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        et_f: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        ambient_f: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        heater: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        fan: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        drum: Option<f64>,
    },
    #[serde(rename_all = "camelCase")]
    Status {
        kind: SourceStatusKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        at_session_sec: f64,
    },
    /// CONTRACTS.md §6 — emitted whenever the BACKEND changes canonical markers
    /// outside a `mark_event` call (v0.1.0: auto CHARGE/DROP, `auto: true`).
    /// The UI applies it exactly like a `mark_event` response.
    #[serde(rename_all = "camelCase")]
    Marker {
        kind: RoastEventKind,
        auto: bool,
        at_session_sec: f64,
        markers: RoastMarkersDto,
    },
}

// ---------------------------------------------------------------------------
// Sources
// ---------------------------------------------------------------------------

// Simulator/Device are contract variants (types.ts SourceInfo.kind); Phase 1
// only constructs Replay — device sources land in Phase 2a.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Simulator,
    Replay,
    Device,
}

/// Mirrors `SourceInfo` in types.ts.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub id: String,
    pub label: String,
    pub kind: SourceKind,
}

// ---------------------------------------------------------------------------
// Events / markers
// ---------------------------------------------------------------------------

/// Mirrors `RoastEventKind` in types.ts. Stored in `events.kind` as the
/// snake_case string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoastEventKind {
    Charge,
    TurningPoint,
    DryEnd,
    FcStart,
    FcEnd,
    ScStart,
    ScEnd,
    Drop,
    CoolEnd,
    Note,
}

impl RoastEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RoastEventKind::Charge => "charge",
            RoastEventKind::TurningPoint => "turning_point",
            RoastEventKind::DryEnd => "dry_end",
            RoastEventKind::FcStart => "fc_start",
            RoastEventKind::FcEnd => "fc_end",
            RoastEventKind::ScStart => "sc_start",
            RoastEventKind::ScEnd => "sc_end",
            RoastEventKind::Drop => "drop",
            RoastEventKind::CoolEnd => "cool_end",
            RoastEventKind::Note => "note",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "charge" => RoastEventKind::Charge,
            "turning_point" => RoastEventKind::TurningPoint,
            "dry_end" => RoastEventKind::DryEnd,
            "fc_start" => RoastEventKind::FcStart,
            "fc_end" => RoastEventKind::FcEnd,
            "sc_start" => RoastEventKind::ScStart,
            "sc_end" => RoastEventKind::ScEnd,
            "drop" => RoastEventKind::Drop,
            "cool_end" => RoastEventKind::CoolEnd,
            "note" => RoastEventKind::Note,
            _ => return None,
        })
    }
}

/// Mirrors `RoastStatus` in types.ts. Stored in `roasts.status` as the string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RoastStatus {
    Recording,
    Finished,
    Abandoned,
}

impl RoastStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RoastStatus::Recording => "recording",
            RoastStatus::Finished => "finished",
            RoastStatus::Abandoned => "abandoned",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "recording" => RoastStatus::Recording,
            "finished" => RoastStatus::Finished,
            "abandoned" => RoastStatus::Abandoned,
            _ => return None,
        })
    }
}

/// Mirrors `RoastMarkersSec` in types.ts — all `*Sec` are seconds-from-charge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoastMarkersDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turning_point_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turning_point_temp_f: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_end_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fc_start_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fc_end_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop_temp_f: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charge_temp_f: Option<f64>,
}

// ---------------------------------------------------------------------------
// Roast records
// ---------------------------------------------------------------------------

/// Mirrors `RoastSummary` in types.ts (`extends RoastMarkersSec` → flattened).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoastSummaryDto {
    pub roast_uuid: String,
    pub status: RoastStatus,
    pub started_wall_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coffee_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charge_weight_lb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop_weight_lb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight_loss_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dtr: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(flatten)]
    pub markers: RoastMarkersDto,
}

/// Mirrors `LiveSample` in types.ts. Doubles as the raw sample shape the
/// capture sources produce (it is the same shape by contract).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleDto {
    pub seq: u64,
    pub session_sec: f64,
    pub bt_f: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub et_f: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambient_f: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heater: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drum: Option<f64>,
}

impl SampleDto {
    pub fn to_event(self) -> SampleEvent {
        SampleEvent::Sample {
            seq: self.seq,
            session_sec: self.session_sec,
            bt_f: self.bt_f,
            et_f: self.et_f,
            ambient_f: self.ambient_f,
            heater: self.heater,
            fan: self.fan,
            drum: self.drum,
        }
    }
}

/// Mirrors `RoastEvent` in types.ts (raw tap history row).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDto {
    pub kind: RoastEventKind,
    pub session_sec: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

// ---------------------------------------------------------------------------
// Command args / results (CONTRACTS.md §2)
// ---------------------------------------------------------------------------

/// `start_session` meta; also the `meta` inside `RecoveryDto`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetaDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_local_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coffee_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charge_weight_lb: Option<f64>,
    /// v0.1.0 (§5/§8): optional link into `coffees_local`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coffee_local_id: Option<i64>,
    /// v0.1.0 (§5/§8): the background-replay reference roast used, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_roast_uuid: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSessionArgs {
    pub source_id: String,
    #[serde(default)]
    pub speed: Option<f64>,
    #[serde(default)]
    pub meta: SessionMetaDto,
    /// v0.1.0 (§7.2): required for `tc4:` sources; ignored for replay.
    #[serde(default)]
    pub source_pin: Option<SourcePinDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSessionResult {
    pub roast_uuid: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkEventArgs {
    pub roast_uuid: String,
    pub kind: RoastEventKind,
    #[serde(default)]
    pub session_sec: Option<f64>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoEventArgs {
    pub roast_uuid: String,
    pub kind: RoastEventKind,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishSessionArgs {
    pub roast_uuid: String,
    #[serde(default)]
    pub drop_weight_lb: Option<f64>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Args for commands that only reference a roast.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoastRefArgs {
    pub roast_uuid: String,
}

/// `pending_recovery` result — an orphaned `recording` roast from a previous
/// process. `lastSeq` is 0 when the roast has no samples (seq starts at 1).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryDto {
    pub roast_uuid: String,
    pub started_wall_ms: i64,
    pub last_seq: u64,
    pub last_session_sec: f64,
    pub meta: SessionMetaDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeSessionResult {
    pub resumed_from_seq: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetRoastResult {
    pub summary: RoastSummaryDto,
    pub samples: Vec<SampleDto>,
    pub events: Vec<EventDto>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSettingArgs {
    pub key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSettingArgs {
    pub key: String,
    pub value: String,
}

// ---------------------------------------------------------------------------
// v0.1.0 expansion DTOs (CONTRACTS.md §5–§7)
// ---------------------------------------------------------------------------

/// Display temperature unit on the wire (`'F' | 'C'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TempUnitDto {
    F,
    C,
}

/// `machines_local.source_pin` JSON + `start_session.sourcePin` (§5).
/// `btChannel`/`etChannel` are 1-based TC4 logical channels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePinDto {
    /// e.g. `tc4:<portName>`
    pub source_id: String,
    #[serde(default = "default_baud")]
    pub baud: u32,
    #[serde(default = "default_bt_channel")]
    pub bt_channel: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub et_channel: Option<u32>,
    #[serde(default = "default_unit")]
    pub unit: TempUnitDto,
}

fn default_baud() -> u32 {
    115_200
}
fn default_bt_channel() -> u32 {
    1
}
fn default_unit() -> TempUnitDto {
    TempUnitDto::F
}

/// USB-serial bridge chip family, when identifiable from VID/PID (§7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SerialChip {
    Ftdi,
    Cp210x,
    Ch340,
    Other,
}

/// One row of `list_serial_ports` (§7.2). `likely` = looks like a TC4 rig.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortDto {
    pub port_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vid: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chip: Option<SerialChip>,
    pub likely: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SniffPortArgs {
    pub port_name: String,
    /// default 115200
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baud: Option<u32>,
    /// default 3000
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SniffVerdict {
    Tc4,
    Unknown,
    Silent,
}

/// `sniff_port` result (§7.2). `diagnostic` is the copy-pasteable device-report
/// block (port, VID/PID, baud, frames) for the GitHub issue template.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SniffResultDto {
    pub verdict: SniffVerdict,
    pub raw_frames: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<Vec<u32>>,
    pub diagnostic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPortPreviewArgs {
    pub port_name: String,
    /// default 115200
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baud: Option<u32>,
    /// default 'F'
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<TempUnitDto>,
}

/// ~1Hz frame on `Channel<PreviewEvent>` (§7.2); channels in 1-based TC4 order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEvent {
    pub channels: Vec<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambient_f: Option<f64>,
    pub at_ms: i64,
}

/// Args for `preview_import` / `import_roasts` (§7.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPathsArgs {
    pub paths: Vec<String>,
}

/// Per-file parse-only preview (§7.1) — never writes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewDto {
    pub path: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coffee_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_wall_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markers: Option<RoastMarkersDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailureDto {
    pub path: String,
    pub error: String,
}

/// `import_roasts` result (§7.1).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResultDto {
    pub imported: u64,
    pub roast_uuids: Vec<String>,
    pub failed: Vec<ImportFailureDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Alog,
    Csv,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRoastArgs {
    pub roast_uuid: String,
    pub format: ExportFormat,
    /// Chosen by the frontend via the dialog plugin.
    pub dest_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResultDto {
    pub path: String,
}

/// `machines_local` row (§7.3). Machines are never hard-deleted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineDto {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub make: Option<String>,
    /// SourcePin JSON (§5 shape), stored verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_pin_json: Option<String>,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveMachineArgs {
    /// absent = create, present = update
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub make: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_pin_json: Option<String>,
}

/// `coffees_local` row (§7.3).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoffeeDto {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCoffeeArgs {
    /// absent = create, present = update
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Args for `archive_machine` / `archive_coffee` (§7.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalIdArgs {
    pub id: i64,
}

/// `update_roast` patch (§7.3): absent = unchanged, explicit `null` = clear.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoastPatchDto {
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub coffee_name: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub coffee_local_id: Option<Option<i64>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub machine_local_id: Option<Option<i64>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub charge_weight_lb: Option<Option<f64>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub drop_weight_lb: Option<Option<f64>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub notes: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRoastArgs {
    pub roast_uuid: String,
    pub patch: RoastPatchDto,
}

/// `update_roast_markers` patch (§7.3): absent = unchanged, `null` = clear.
/// Charge is NOT editable in v0.1.0; temps recompute from samples.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkerPatchDto {
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub turning_point_sec: Option<Option<f64>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub dry_end_sec: Option<Option<f64>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub fc_start_sec: Option<Option<f64>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub fc_end_sec: Option<Option<f64>>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub drop_sec: Option<Option<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRoastMarkersArgs {
    pub roast_uuid: String,
    pub markers: MarkerPatchDto,
}

/// `get_app_info` result (§7.4).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoDto {
    pub version: String,
    pub os: String,
    pub arch: String,
}

/// `check_for_update` result (§7.4) — notice-only in v0.1.0.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfoDto {
    pub current_version: String,
    pub latest_version: String,
    pub url: String,
    pub is_newer: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_event_serializes_tagged_camel_case() {
        let ev = SampleEvent::Sample {
            seq: 3,
            session_sec: 2.0,
            bt_f: 385.4,
            et_f: Some(410.0),
            ambient_f: None,
            heater: None,
            fan: None,
            drum: None,
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["type"], "sample");
        assert_eq!(json["seq"], 3);
        assert_eq!(json["sessionSec"], 2.0);
        assert_eq!(json["btF"], 385.4);
        assert_eq!(json["etF"], 410.0);
        assert!(
            json.get("ambientF").is_none(),
            "None fields must be omitted"
        );

        let st = SampleEvent::Status {
            kind: SourceStatusKind::Reconnected,
            message: None,
            at_session_sec: 12.0,
        };
        let json = serde_json::to_value(&st).unwrap();
        assert_eq!(json["type"], "status");
        assert_eq!(json["kind"], "reconnected");
        assert_eq!(json["atSessionSec"], 12.0);
    }

    #[test]
    fn summary_flattens_markers() {
        let s = RoastSummaryDto {
            roast_uuid: "u".into(),
            status: RoastStatus::Finished,
            started_wall_ms: 1,
            machine_name: None,
            coffee_name: None,
            charge_weight_lb: None,
            drop_weight_lb: None,
            weight_loss_pct: None,
            dtr: Some(0.21),
            notes: None,
            markers: RoastMarkersDto {
                drop_sec: Some(660.0),
                ..Default::default()
            },
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["status"], "finished");
        assert_eq!(json["dropSec"], 660.0);
        assert_eq!(json["dtr"], 0.21);
        assert!(
            json.get("markers").is_none(),
            "markers must flatten into the summary"
        );
    }

    #[test]
    fn event_kind_round_trips() {
        for kind in [
            RoastEventKind::Charge,
            RoastEventKind::TurningPoint,
            RoastEventKind::DryEnd,
            RoastEventKind::FcStart,
            RoastEventKind::FcEnd,
            RoastEventKind::ScStart,
            RoastEventKind::ScEnd,
            RoastEventKind::Drop,
            RoastEventKind::CoolEnd,
            RoastEventKind::Note,
        ] {
            assert_eq!(RoastEventKind::parse(kind.as_str()), Some(kind));
            // serde string form must match the DB string form
            let json = serde_json::to_value(kind).unwrap();
            assert_eq!(json.as_str().unwrap(), kind.as_str());
        }
    }

    #[test]
    fn logger_error_serializes_code_message() {
        let err = crate::error::LoggerError::session_active();
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "session_active");
        assert!(json["message"].as_str().unwrap().contains("one session"));

        let err = crate::error::LoggerError::invalid_args("missing sourcePin");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "invalid_args");
    }

    #[test]
    fn marker_event_serializes_per_contract_section_6() {
        let ev = SampleEvent::Marker {
            kind: RoastEventKind::Charge,
            auto: true,
            at_session_sec: 62.0,
            markers: RoastMarkersDto {
                charge_temp_f: Some(388.0),
                ..Default::default()
            },
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["type"], "marker");
        assert_eq!(json["kind"], "charge");
        assert_eq!(json["auto"], true);
        assert_eq!(json["atSessionSec"], 62.0);
        // markers is a NESTED object here (unlike RoastSummaryDto's flatten)
        assert_eq!(json["markers"]["chargeTempF"], 388.0);
        assert!(
            json["markers"].get("dropSec").is_none(),
            "None marker fields omitted"
        );
    }

    #[test]
    fn double_option_patch_absent_null_and_value() {
        // value → Some(Some(v)); explicit null → Some(None); absent → None.
        let patch: RoastPatchDto =
            serde_json::from_str(r#"{ "coffeeName": "Guji", "notes": null }"#).unwrap();
        assert_eq!(patch.coffee_name, Some(Some("Guji".to_string())));
        assert_eq!(patch.notes, Some(None));
        assert_eq!(
            patch.charge_weight_lb, None,
            "absent field must mean 'unchanged'"
        );
        assert_eq!(patch.coffee_local_id, None);

        // Round-trips: unchanged fields stay absent, cleared fields stay null.
        let json = serde_json::to_value(&patch).unwrap();
        assert_eq!(json["coffeeName"], "Guji");
        assert!(json["notes"].is_null());
        assert!(json.get("chargeWeightLb").is_none());

        let markers: MarkerPatchDto =
            serde_json::from_str(r#"{ "dropSec": 612.5, "turningPointSec": null }"#).unwrap();
        assert_eq!(markers.drop_sec, Some(Some(612.5)));
        assert_eq!(markers.turning_point_sec, Some(None));
        assert_eq!(markers.dry_end_sec, None);
    }

    #[test]
    fn source_pin_defaults_and_camel_case() {
        let pin: SourcePinDto =
            serde_json::from_str(r#"{ "sourceId": "tc4:usbserial-1420" }"#).unwrap();
        assert_eq!(pin.baud, 115_200);
        assert_eq!(pin.bt_channel, 1);
        assert_eq!(pin.et_channel, None);
        assert_eq!(pin.unit, TempUnitDto::F);

        let pin: SourcePinDto = serde_json::from_str(
            r#"{ "sourceId": "tc4:COM3", "baud": 57600, "btChannel": 2, "etChannel": 1, "unit": "C" }"#,
        )
        .unwrap();
        assert_eq!(pin.bt_channel, 2);
        assert_eq!(pin.et_channel, Some(1));
        assert_eq!(pin.unit, TempUnitDto::C);
        let json = serde_json::to_value(&pin).unwrap();
        assert_eq!(json["sourceId"], "tc4:COM3");
        assert_eq!(json["btChannel"], 2);
        assert_eq!(json["unit"], "C");
    }
}
