/**
 * THE CONTRACT — shared types for the entire roast-console system.
 *
 * Canonical units (binding, matches the Droptime platform):
 *   temperature °F  ·  time-in-roast seconds  ·  wall/mono clocks ms
 *
 * Time model: a session starts recording BEFORE charge (preheat). Samples carry
 * `sessionSec` (seconds since recording started, from the capture source's
 * monotonic clock). The CHARGE event pins `chargeSessionSec` on the session;
 * chart-time is t = sessionSec - chargeSessionSec (negative during preheat).
 * Everything synced/exported is rebased to seconds-from-charge.
 */

// ---------- samples ----------

export interface LiveSample {
  /** per-session monotone sequence from the capture source (gap-detectable) */
  seq: number;
  /** seconds since recording started (source monotonic clock) */
  sessionSec: number;
  btF: number;
  etF?: number;
  ambientF?: number;
  /** control-channel telemetry, 0-100 when the rig reports it */
  heater?: number;
  fan?: number;
  drum?: number;
}

export type SourceStatusKind =
  | 'connected'
  | 'disconnected'
  | 'reconnected'
  | 'flatline' // probe reads identical values long enough to be suspicious
  | 'gap' // seq discontinuity detected
  | 'ended'; // replay fixture finished

export interface SourceStatus {
  kind: SourceStatusKind;
  message?: string;
  atSessionSec: number;
}

// ---------- sources (implemented by SimulatorSource in this package and by
// the Tauri bridge in apps/droptime-logger; the console cannot tell them apart) ----------

export interface SourceInfo {
  id: string;
  label: string;
  kind: 'simulator' | 'replay' | 'device';
}

export interface SourceListener {
  onSample(sample: LiveSample): void;
  onStatus(status: SourceStatus): void;
}

export interface SampleSource {
  readonly info: SourceInfo;
  start(listener: SourceListener): Promise<void>;
  stop(): Promise<void>;
}

// ---------- events / markers ----------

export type RoastEventKind =
  | 'charge'
  | 'turning_point'
  | 'dry_end'
  | 'fc_start'
  | 'fc_end'
  | 'sc_start'
  | 'sc_end'
  | 'drop'
  | 'cool_end'
  | 'note';

export interface RoastEvent {
  kind: RoastEventKind;
  sessionSec: number;
  note?: string;
}

// ---------- session / roast records ----------

export type RoastStatus = 'recording' | 'finished' | 'abandoned';

export interface TargetProfile {
  name: string;
  targetChargeTempF?: number;
  targetDropTimeSec?: number;
  targetFcStartSec?: number;
  targetDtr?: number;
  /** t is seconds-from-charge, matching platform roastProfiles.targetCurve */
  targetCurve?: Array<{ t: number; bt: number; ror?: number }>;
}

export interface RoastSessionMeta {
  roastUuid: string;
  startedWallMs: number;
  machineName?: string;
  coffeeName?: string;
  chargeWeightLb?: number;
  targetProfile?: TargetProfile;
}

/** Post-charge chart point (rebased). ror is display-derived, never stored raw. */
export interface CurvePoint {
  t: number;
  bt: number;
  et?: number;
  ror?: number;
  heater?: number;
  fan?: number;
  drum?: number;
}

export interface RoastMarkersSec {
  /** all seconds-from-charge; undefined = not (yet) marked */
  turningPointSec?: number;
  turningPointTempF?: number;
  dryEndSec?: number;
  fcStartSec?: number;
  fcEndSec?: number;
  dropSec?: number;
  dropTempF?: number;
  chargeTempF?: number;
}

export interface RoastSummary extends RoastMarkersSec {
  roastUuid: string;
  status: RoastStatus;
  startedWallMs: number;
  machineName?: string;
  coffeeName?: string;
  chargeWeightLb?: number;
  dropWeightLb?: number;
  weightLossPct?: number;
  dtr?: number;
  notes?: string;
}

// ---------- live derived state (what the console renders) ----------

export type RoastPhase = 'preheat' | 'drying' | 'maillard' | 'development' | 'cooling' | 'done';

export interface PhaseBreakdown {
  phase: RoastPhase;
  /** percentages of time-since-charge; present once charge is marked */
  dryingPct?: number;
  maillardPct?: number;
  developmentPct?: number;
  /** live DTR once fc_start exists */
  dtr?: number;
}

export interface DropProjection {
  /** projected seconds-from-charge at which target drop temp is reached */
  projectedDropSec: number;
  targetDropTempF: number;
  confidence: 'low' | 'medium' | 'high';
}

// ---------- simulator fixtures ----------

/** JSON fixture format replayed by both the TS simulator and the Rust replay driver. */
export interface RoastFixture {
  name: string;
  description?: string;
  /** original sampling interval of the recorded/synthesized data */
  sampleIntervalSec: number;
  /** t is seconds-from-charge; preheat samples may use negative t */
  curve: Array<{ t: number; bt: number; et?: number }>;
  markers: RoastMarkersSec & { chargeWeightLb?: number; coffeeName?: string };
}

export interface SimulatorOptions {
  /** playback speed multiplier, 1–20 */
  speed?: number;
  /** gaussian noise stddev in °F applied to bt/et (default 0 = clean) */
  noiseF?: number;
  /** probability per sample of a dropped sample (seq gap), default 0 */
  dropoutP?: number;
  /** emit interval override in seconds of roast-time (default fixture's) */
  sampleIntervalSec?: number;
  /** start at charge (skip preheat), default true */
  startAtCharge?: boolean;
  /** loop when fixture ends, default false */
  loop?: boolean;
}
