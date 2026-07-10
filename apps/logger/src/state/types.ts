import type {
  CurvePoint,
  DropProjection,
  LiveSample,
  PhaseBreakdown,
  RoastEventKind,
  RoastMarkersSec,
  RoastSummary,
  TargetProfile,
} from '@droptime/roast-console';
import type { AppMode } from '../bridge';

export type Screen = 'setup' | 'live' | 'summary' | 'history' | 'detail' | 'settings' | 'wizard';

/** Optional payload carried by NAVIGATE (CONTRACTS §8), e.g. detail's roast. */
export interface NavParams {
  roastUuid?: string;
}

/**
 * Background-replay reference (CONTRACTS §8): the prior roast the live session
 * is roasted against. Set via `actions.setReference`; consumed by the
 * reference feature (ghost curve + cue layer).
 */
export interface ReferenceState {
  roastUuid: string;
  label: string;
  /** chart-ready curve (t = seconds-from-charge) */
  curve: CurvePoint[];
  markers: RoastMarkersSec;
}

export type BannerTone = 'warn' | 'good' | 'neutral';

export interface Banner {
  /** stable per-condition id so repeated status events dedupe into one banner */
  id: string;
  tone: BannerTone;
  text: string;
}

export type ConnState =
  | 'idle'
  | 'connected'
  | 'reconnected'
  | 'gap'
  | 'flatline'
  | 'disconnected'
  | 'ended';

/** Everything the setup screen collected plus resolved source display info. */
export interface SessionMeta {
  sourceId: string;
  sourceLabel: string;
  speed: number;
  coffeeName?: string;
  chargeWeightLb?: number;
  machineName?: string;
  /** v0.1.0 (§8): optional link into coffees_local (persisted at start_session). */
  coffeeLocalId?: number;
  /** v0.1.0 (§8): background-replay reference used (persisted at start_session). */
  referenceRoastUuid?: string;
}

export interface SessionState {
  screen: Screen;
  /** payload of the last NAVIGATE (e.g. which roast the detail screen shows) */
  navParams?: NavParams;
  mode: AppMode;

  roastUuid?: string;
  startedWallMs?: number;
  meta?: SessionMeta;

  /** background-replay reference roast, null when none selected (§8) */
  reference: ReferenceState | null;

  /** canonical markers (seconds-from-charge), authoritative from backend in Tauri */
  markers: RoastMarkersSec;
  /** absolute session-seconds at which charge was pinned; drives rebasing */
  chargeSessionSec?: number;
  /** ordered stack of applied marks, for keyboard "undo last" */
  markHistory: RoastEventKind[];

  banners: Banner[];
  connection: ConnState;

  /** optional target profile (no selection UI in Phase 1 — plumbing stays live) */
  target: TargetProfile | null;

  /** authoritative summary after finish_session (Tauri) / local finalize (browser) */
  finalSummary?: RoastSummary;
  finishing: boolean;
}

/** Live derived state recomputed at <=1Hz from the sample ring buffer. */
export interface DerivedState {
  points: CurvePoint[];
  latestSample?: LiveSample;
  elapsedT: number;
  charged: boolean;
  ror?: number;
  phase: PhaseBreakdown;
  projection: DropProjection | null;
}

export type SessionAction =
  | { type: 'RESET' }
  | {
      type: 'SESSION_STARTED';
      roastUuid: string;
      startedWallMs: number;
      meta: SessionMeta;
    }
  | {
      type: 'RESUMED';
      roastUuid: string;
      startedWallMs: number;
      meta: SessionMeta;
      markers: RoastMarkersSec;
      chargeSessionSec?: number;
      markHistory: RoastEventKind[];
    }
  | {
      type: 'APPLY_MARKERS';
      markers: RoastMarkersSec;
      chargeSessionSec?: number;
      markHistory: RoastEventKind[];
    }
  | { type: 'STATUS'; kind: ConnState; message?: string }
  | { type: 'DISMISS_BANNER'; id: string }
  | { type: 'PUSH_BANNER'; banner: Banner }
  | { type: 'NAVIGATE'; screen: Screen; params?: NavParams }
  | { type: 'SET_REFERENCE'; reference: ReferenceState | null }
  | { type: 'SET_FINISHING'; finishing: boolean }
  | { type: 'FINALIZED'; summary: RoastSummary };
