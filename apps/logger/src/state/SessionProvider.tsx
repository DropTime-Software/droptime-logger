import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import {
  BUNDLED_FIXTURES,
  SimulatorSource,
  phaseBreakdown,
  projectDrop,
  rebaseSamples,
  trailingRoR,
  type LiveSample,
  type RoastEventKind,
  type RoastFixture,
  type RoastMarkersSec,
  type RoastSummary,
  type SampleSource,
  type SourceInfo,
  type SourceListener,
} from '@droptime/roast-console';
import {
  TauriSampleSource,
  ipc,
  type AppMode,
  type MarkerEvent,
  type RecoveryDto,
  type SourcePin,
  type StartSessionArgs,
} from '../bridge';
import { initialState, sessionReducer } from './reducer';
import {
  KEY_TO_KIND,
  applyBrowserMark,
  buildLiveSummary,
  clearBrowserMark,
  historyFromEvents,
  isMarked,
  nearestSample,
  nextExpected,
} from './roast';
import type {
  DerivedState,
  NavParams,
  ReferenceState,
  SessionMeta,
  SessionState,
} from './types';

const MAX_SAMPLES = 60_000; // ~16h at 1Hz — a hard ring-buffer bound
const GAP_BANNER_MS = 6_000;
const FLATLINE_BANNER_MS = 8_000;
const RECONNECTED_BANNER_MS = 4_000;
/**
 * Post-drop capture window in ROAST seconds. Capture deliberately continues
 * past DROP — cool-down data is kept and an accidental drop-tap can be undone
 * losslessly — but it is BOUNDED and visible (SummaryScreen chip), then the
 * source actually stops. Undoing drop inside the window cancels the stop.
 */
const COOL_DOWN_ROAST_SEC = 60;
/** Auto-mark banners self-dismiss after this (undo stays available). */
const AUTO_MARK_BANNER_MS = 8_000;

/** kind → its keyboard key, for the auto-mark undo hint. */
const KIND_TO_KEY: Partial<Record<RoastEventKind, string>> = Object.fromEntries(
  Object.entries(KEY_TO_KIND).map(([key, kind]) => [kind, key.toUpperCase()] as const),
);

export interface StartConfig {
  sourceId: string;
  sourceLabel: string;
  /** browser mode: key into BUNDLED_FIXTURES */
  fixtureName?: string;
  speed: number;
  coffeeName?: string;
  chargeWeightLb?: number;
  machineName?: string;
  /** v0.1.0 (§8): library links + reference, persisted at start_session. */
  coffeeLocalId?: number;
  machineLocalId?: number;
  referenceRoastUuid?: string;
  /** v0.1.0 (§7.2): required for tc4: sources. */
  sourcePin?: SourcePin;
}

/** Wall-clock arrival stamp of the newest sample (display interpolation only). */
export interface SampleArrival {
  sample: LiveSample;
  /** performance.now() at the moment the sample landed in the store */
  wallMs: number;
}

export interface SessionContextValue {
  state: SessionState;
  derived: DerivedState;
  /** whether a capture source is currently streaming */
  capturing: boolean;
  /**
   * Read-only accessor for the newest sample + its wall-arrival stamp.
   * Ref-backed (never re-renders); the LiveScreen rAF loop polls it per frame
   * to extrapolate a smooth leading edge. Null until the first sample and
   * after stop/reset.
   */
  getLatestArrival(): SampleArrival | null;
  /** Read-only accessor for the raw sample ring buffer (ref-backed). */
  getSamples(): readonly LiveSample[];
  /** Active post-drop cool-down window (null once capture has stopped). */
  coolDown: { endsWallMs: number } | null;
  actions: {
    startRoast(cfg: StartConfig): Promise<void>;
    resumeRoast(recovery: RecoveryDto): Promise<void>;
    markEvent(kind: RoastEventKind, sessionSecOverride?: number): Promise<void>;
    markNext(): Promise<void>;
    undoEvent(kind: RoastEventKind): Promise<void>;
    undoLast(): Promise<void>;
    finishRoast(dropWeightLb?: number, notes?: string): Promise<void>;
    abandonRoast(): Promise<void>;
    navigate(screen: SessionState['screen'], params?: NavParams): void;
    startAnother(): Promise<void>;
    /** End the cool-down window now: stop streaming, keep the roast unfinalized. */
    stopCaptureNow(): Promise<void>;
    previewSummary(dropWeightLb?: number, notes?: string): RoastSummary;
    /** Select/clear the background-replay reference (CONTRACTS §8). */
    setReference(reference: ReferenceState | null): void;
  };
}

const SessionContext = createContext<SessionContextValue | null>(null);

export function useSession(): SessionContextValue {
  const ctx = useContext(SessionContext);
  if (!ctx) throw new Error('useSession must be used within <SessionProvider>');
  return ctx;
}

export function SessionProvider({ mode, children }: { mode: AppMode; children: ReactNode }) {
  const [state, dispatch] = useReducer(sessionReducer, mode, initialState);
  const [capturing, setCapturing] = useState(false);
  const [tick, setTick] = useState(0);

  // High-frequency data lives in refs so sample ingestion never re-renders.
  const samplesRef = useRef<LiveSample[]>([]);
  const maxSeqRef = useRef(-1);
  const sourceRef = useRef<SampleSource | null>(null);
  // Wall-arrival stamp of the newest sample — read by the LiveScreen rAF loop.
  const latestArrivalRef = useRef<SampleArrival | null>(null);

  // Latest state, readable inside async action closures without staleness.
  const stateRef = useRef(state);
  useEffect(() => {
    stateRef.current = state;
  }, [state]);

  // Capturing flag readable inside async action closures (mirrors stateRef).
  const capturingRef = useRef(capturing);
  useEffect(() => {
    capturingRef.current = capturing;
  }, [capturing]);

  const resetSamples = useCallback(() => {
    samplesRef.current = [];
    maxSeqRef.current = -1;
    latestArrivalRef.current = null;
  }, []);

  const bumpTick = useCallback(() => setTick((t) => t + 1), []);

  // Replay sources know exactly when charge happens (fixture t=0), so the
  // provider auto-marks CHARGE the moment the replay crosses it — at 20x a
  // human has ~3 real seconds, which is not an input surface. Device sources
  // (Phase 2a) never auto-mark: the roaster taps.
  const autoChargeAtRef = useRef<number | null>(null);
  const autoChargeFireRef = useRef<((at: number) => void) | null>(null);

  // Backend-initiated marker changes (§6 `marker` stream events — v0.1.0 auto
  // CHARGE/DROP). Ref-pointed at the live handler like autoChargeFireRef.
  const backendMarkerRef = useRef<((ev: MarkerEvent) => void) | null>(null);

  // Post-drop cool-down window (bounded capture past DROP; see COOL_DOWN_ROAST_SEC).
  const [coolDown, setCoolDown] = useState<{ endsWallMs: number } | null>(null);
  const coolDownTimerRef = useRef<number | null>(null);
  const clearCoolDown = useCallback(() => {
    if (coolDownTimerRef.current != null) {
      window.clearTimeout(coolDownTimerRef.current);
      coolDownTimerRef.current = null;
    }
    setCoolDown(null);
  }, []);
  // The 'ended' status (replay exhausted) also closes the live feed.
  const endedFireRef = useRef<(() => void) | null>(null);

  // Stable listener bridging any SampleSource into the store.
  const listener = useRef<SourceListener>({
    onSample(s) {
      const arr = samplesRef.current;
      const chargeAt = autoChargeAtRef.current;
      if (chargeAt != null && s.sessionSec >= chargeAt) {
        autoChargeAtRef.current = null;
        autoChargeFireRef.current?.(chargeAt);
      }
      if (s.seq > maxSeqRef.current) {
        arr.push(s);
        maxSeqRef.current = s.seq;
        // Stamp wall arrival of the newest sample for display interpolation.
        latestArrivalRef.current = { sample: s, wallMs: performance.now() };
        if (arr.length > MAX_SAMPLES) arr.splice(0, arr.length - MAX_SAMPLES);
        if (arr.length === 1) bumpTick(); // paint the first point immediately
      } else {
        // Out-of-order / duplicate (e.g. resume overlap): idempotent upsert.
        const i = arr.findIndex((x) => x.seq === s.seq);
        if (i >= 0) arr[i] = s;
        else {
          arr.push(s);
          arr.sort((a, b) => a.seq - b.seq);
        }
      }
    },
    onStatus(st) {
      dispatch({ type: 'STATUS', kind: st.kind, message: st.message });
      if (st.kind === 'ended') endedFireRef.current?.();
      if (st.kind === 'gap') {
        window.setTimeout(() => dispatch({ type: 'DISMISS_BANNER', id: 'gap' }), GAP_BANNER_MS);
      } else if (st.kind === 'flatline') {
        window.setTimeout(
          () => dispatch({ type: 'DISMISS_BANNER', id: 'flatline' }),
          FLATLINE_BANNER_MS,
        );
      } else if (st.kind === 'reconnected') {
        window.setTimeout(
          () => dispatch({ type: 'DISMISS_BANNER', id: 'reconnected' }),
          RECONNECTED_BANNER_MS,
        );
      }
    },
  });

  // Derived recompute cadence: <=1Hz while capturing (plus first-sample kick).
  useEffect(() => {
    if (!capturing) return;
    const id = window.setInterval(bumpTick, 1000);
    return () => window.clearInterval(id);
  }, [capturing, bumpTick]);

  const derived = useMemo<DerivedState>(() => {
    const samples = samplesRef.current;
    const cs = state.chargeSessionSec;
    const charged = cs != null;
    const points = rebaseSamples(samples, cs);
    const latestSample = samples.length > 0 ? samples[samples.length - 1] : undefined;
    const elapsedT = cs != null && latestSample ? latestSample.sessionSec - cs : 0;
    const ror = trailingRoR(points);
    const phase = phaseBreakdown(state.markers, elapsedT, charged);
    // Phase 1 has no drop-temperature target (setup collects none; TargetProfile
    // carries a target drop *time*, not temp) so projection is null. The
    // projectDrop call stays wired for when target profiles land.
    const targetDropTempF: number | undefined = undefined;
    const projection = targetDropTempF != null ? (projectDrop(points, targetDropTempF) ?? null) : null;
    return { points, latestSample, elapsedT, charged, ror, phase, projection };
    // tick forces the throttled recompute; markers/charge recompute eagerly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, state.markers, state.chargeSessionSec]);

  const stopSource = useCallback(async () => {
    setCapturing(false);
    autoChargeAtRef.current = null;
    latestArrivalRef.current = null;
    clearCoolDown();
    const src = sourceRef.current;
    sourceRef.current = null;
    if (src) {
      try {
        await src.stop();
      } catch {
        /* best-effort */
      }
    }
  }, [clearCoolDown]);

  /** End of the cool-down window: stop streaming, roast stays unfinalized. */
  const stopCaptureNow = useCallback(async () => {
    const st = stateRef.current;
    if (st.mode === 'tauri' && st.roastUuid && !st.finalSummary) {
      try {
        await ipc.stopCapture({ roastUuid: st.roastUuid });
      } catch {
        /* engine may already be stopped */
      }
    }
    await stopSource();
  }, [stopSource]);

  useEffect(() => {
    endedFireRef.current = () => {
      clearCoolDown();
      setCapturing(false);
    };
  }, [clearCoolDown]);

  /**
   * Begin the bounded post-drop capture window (roast-time scaled to wall
   * time) and land on the summary. Shared by the manual drop mark and the §6
   * auto-detected drop marker.
   */
  const beginDropCoolDown = useCallback(() => {
    const st = stateRef.current;
    const speed = st.meta?.speed ?? 1;
    const windowMs = (COOL_DOWN_ROAST_SEC / speed) * 1000;
    setCoolDown({ endsWallMs: performance.now() + windowMs });
    coolDownTimerRef.current = window.setTimeout(() => {
      coolDownTimerRef.current = null;
      void stopCaptureNow();
    }, windowMs);
    dispatch({ type: 'NAVIGATE', screen: 'summary' });
  }, [stopCaptureNow]);

  /**
   * Apply a §6 `marker` stream event exactly like a mark_event response:
   * markers are backend-authoritative, the kind joins markHistory (so
   * "undo last" covers it), and auto marks surface an undo banner.
   */
  const applyBackendMarker = useCallback(
    (ev: MarkerEvent) => {
      const st = stateRef.current;
      if (!st.roastUuid || st.finalSummary) return;

      const nextChargeSessionSec =
        ev.kind === 'charge' ? ev.atSessionSec : st.chargeSessionSec;
      dispatch({
        type: 'APPLY_MARKERS',
        markers: ev.markers,
        chargeSessionSec: nextChargeSessionSec,
        markHistory: [...st.markHistory, ev.kind],
      });

      if (ev.auto) {
        const id = `auto-mark-${ev.kind}`;
        const key = KIND_TO_KEY[ev.kind];
        const label = ev.kind.replace(/_/g, ' ').toUpperCase();
        dispatch({
          type: 'PUSH_BANNER',
          banner: {
            id,
            tone: 'good',
            text: `${label} auto-detected${key ? ` — press ${key} to undo` : ''}`,
          },
        });
        window.setTimeout(() => dispatch({ type: 'DISMISS_BANNER', id }), AUTO_MARK_BANNER_MS);
      }

      if (ev.kind === 'drop') beginDropCoolDown();
    },
    [beginDropCoolDown],
  );

  useEffect(() => {
    backendMarkerRef.current = applyBackendMarker;
  }, [applyBackendMarker]);

  // ---- actions ----

  const startRoast = useCallback(
    async (cfg: StartConfig) => {
      // Guard: never detach a still-recording session — stopSource() wipes the
      // live curve and the backend would then reject with session_active. Any
      // path that reaches here mid-roast must finish or drop first.
      if (capturingRef.current && stateRef.current.roastUuid && !stateRef.current.finalSummary) {
        throw new Error('A roast is already in progress — finish or drop it first.');
      }
      await stopSource();
      resetSamples();
      const startedWallMs = Date.now();
      const meta: SessionMeta = {
        sourceId: cfg.sourceId,
        sourceLabel: cfg.sourceLabel,
        speed: cfg.speed,
        coffeeName: cfg.coffeeName?.trim() || undefined,
        chargeWeightLb: cfg.chargeWeightLb,
        machineName: cfg.machineName?.trim() || undefined,
        coffeeLocalId: cfg.coffeeLocalId,
        referenceRoastUuid: cfg.referenceRoastUuid,
      };

      let roastUuid: string;
      let source: SampleSource;

      if (mode === 'tauri') {
        const args: StartSessionArgs = {
          sourceId: cfg.sourceId,
          speed: cfg.speed,
          meta: {
            coffeeName: meta.coffeeName,
            chargeWeightLb: meta.chargeWeightLb,
            machineLocalId: cfg.machineLocalId,
            coffeeLocalId: cfg.coffeeLocalId,
            referenceRoastUuid: cfg.referenceRoastUuid,
          },
          sourcePin: cfg.sourcePin,
        };
        const isDevice = cfg.sourceId.startsWith('tc4:');
        const info: SourceInfo = {
          id: cfg.sourceId,
          label: cfg.sourceLabel,
          kind: isDevice ? 'device' : 'replay',
        };
        const ts = new TauriSampleSource(info, args);
        // §6 marker events (backend auto CHARGE/DROP) — applied like mark_event.
        ts.onMarker = (ev) => backendMarkerRef.current?.(ev);
        // Rust replay maps fixture t=0 (charge) to sessionSec 0 — auto-mark at
        // the first sample. Device sources never front-end auto-charge; the
        // backend detector (§7.2) or the roaster's tap owns it.
        autoChargeAtRef.current = info.kind === 'replay' ? 0 : null;
        await ts.start(listener.current);
        if (!ts.roastUuid) throw new Error('start_session returned no roastUuid');
        roastUuid = ts.roastUuid;
        source = ts;
      } else {
        const fixture = cfg.fixtureName ? BUNDLED_FIXTURES[cfg.fixtureName] : undefined;
        if (!fixture) throw new Error(`Unknown fixture: ${cfg.fixtureName}`);
        const sim = new SimulatorSource(fixture, { speed: cfg.speed, startAtCharge: false });
        roastUuid = crypto.randomUUID();
        autoChargeAtRef.current = sim.chargeAtSessionSec;
        await sim.start(listener.current);
        source = sim;
      }

      sourceRef.current = source;
      setCapturing(true);
      dispatch({ type: 'SESSION_STARTED', roastUuid, startedWallMs, meta });
    },
    [mode, stopSource, resetSamples],
  );

  const resumeRoast = useCallback(
    async (rec: RecoveryDto) => {
      // Only meaningful in Tauri; hydrate the already-captured curve, then reattach.
      await stopSource();
      resetSamples();
      const { summary, samples, events } = await ipc.getRoast({ roastUuid: rec.roastUuid });

      samplesRef.current = [...samples].sort((a, b) => a.seq - b.seq);
      maxSeqRef.current = samplesRef.current.reduce((mx, s) => Math.max(mx, s.seq), -1);

      const chargeEvent = events.find((e) => e.kind === 'charge');
      const meta: SessionMeta = {
        sourceId: 'resumed',
        sourceLabel: summary.coffeeName ?? rec.meta.coffeeName ?? 'Resumed roast',
        speed: 1,
        coffeeName: summary.coffeeName ?? rec.meta.coffeeName,
        chargeWeightLb: summary.chargeWeightLb ?? rec.meta.chargeWeightLb,
        machineName: summary.machineName ?? rec.meta.machineName,
      };
      const markers: RoastMarkersSec = {
        turningPointSec: summary.turningPointSec,
        turningPointTempF: summary.turningPointTempF,
        dryEndSec: summary.dryEndSec,
        fcStartSec: summary.fcStartSec,
        fcEndSec: summary.fcEndSec,
        dropSec: summary.dropSec,
        dropTempF: summary.dropTempF,
        chargeTempF: summary.chargeTempF,
      };

      const info: SourceInfo = { id: meta.sourceId, label: meta.sourceLabel, kind: 'replay' };
      const source = new TauriSampleSource(
        info,
        { sourceId: meta.sourceId, meta: {} },
        rec.roastUuid,
      );
      source.onMarker = (ev) => backendMarkerRef.current?.(ev);
      await source.start(listener.current);
      sourceRef.current = source;
      setCapturing(true);
      bumpTick();

      dispatch({
        type: 'RESUMED',
        roastUuid: rec.roastUuid,
        startedWallMs: summary.startedWallMs ?? rec.startedWallMs,
        meta,
        markers,
        chargeSessionSec: chargeEvent?.sessionSec,
        markHistory: historyFromEvents(events),
      });
    },
    [stopSource, resetSamples, bumpTick],
  );

  const markEvent = useCallback(async (kind: RoastEventKind, sessionSecOverride?: number) => {
    const st = stateRef.current;
    if (!st.roastUuid || st.finalSummary) return;
    const samples = samplesRef.current;
    const latest = samples.length > 0 ? samples[samples.length - 1] : undefined;
    const sessionSec = sessionSecOverride ?? (latest ? latest.sessionSec : 0);
    const charged = st.chargeSessionSec != null;

    // Non-charge marks require a charge first; ignore duplicate marks.
    if (kind !== 'charge' && !charged) return;
    if (isMarked(st.markers, kind, charged)) return;

    const nextChargeSessionSec = kind === 'charge' ? sessionSec : st.chargeSessionSec;

    let markers: RoastMarkersSec;
    if (st.mode === 'tauri') {
      markers = await ipc.markEvent({ roastUuid: st.roastUuid, kind, sessionSec });
    } else {
      markers = applyBrowserMark(
        st.markers,
        kind,
        sessionSec,
        nextChargeSessionSec,
        nearestSample(samples, sessionSec),
      );
    }

    dispatch({
      type: 'APPLY_MARKERS',
      markers,
      chargeSessionSec: nextChargeSessionSec,
      markHistory: [...st.markHistory, kind],
    });

    // Begin the bounded cool-down window and land on the summary.
    if (kind === 'drop') beginDropCoolDown();
  }, [beginDropCoolDown]);

  // Keep the listener's auto-charge trigger pointed at the live markEvent.
  useEffect(() => {
    autoChargeFireRef.current = (at) => void markEvent('charge', at);
  }, [markEvent]);

  const undoEvent = useCallback(async (kind: RoastEventKind) => {
    const st = stateRef.current;
    if (!st.roastUuid || st.finalSummary) return;

    // Undoing drop inside the cool-down window resumes normal live capture.
    if (kind === 'drop') clearCoolDown();

    let markers: RoastMarkersSec;
    if (st.mode === 'tauri') {
      markers = await ipc.undoEvent({ roastUuid: st.roastUuid, kind });
    } else {
      markers = clearBrowserMark(st.markers, kind);
    }
    const nextChargeSessionSec = kind === 'charge' ? undefined : st.chargeSessionSec;
    const idx = st.markHistory.lastIndexOf(kind);
    const markHistory =
      idx >= 0
        ? [...st.markHistory.slice(0, idx), ...st.markHistory.slice(idx + 1)]
        : st.markHistory;

    dispatch({
      type: 'APPLY_MARKERS',
      markers,
      chargeSessionSec: nextChargeSessionSec,
      markHistory,
    });
  }, []);

  const markNext = useCallback(async () => {
    const st = stateRef.current;
    const kind = nextExpected(st.markers, st.chargeSessionSec != null);
    if (kind) await markEvent(kind);
  }, [markEvent]);

  const undoLast = useCallback(async () => {
    const st = stateRef.current;
    const last = st.markHistory[st.markHistory.length - 1];
    if (last) await undoEvent(last);
  }, [undoEvent]);

  const previewSummary = useCallback((dropWeightLb?: number, notes?: string): RoastSummary => {
    const st = stateRef.current;
    if (st.finalSummary) return st.finalSummary;
    const points = rebaseSamples(samplesRef.current, st.chargeSessionSec);
    return buildLiveSummary({
      roastUuid: st.roastUuid ?? '',
      startedWallMs: st.startedWallMs ?? Date.now(),
      meta: st.meta,
      markers: st.markers,
      points,
      dropWeightLb,
      notes,
    });
  }, []);

  const finishRoast = useCallback(
    async (dropWeightLb?: number, notes?: string) => {
      const st = stateRef.current;
      if (!st.roastUuid || st.finishing || st.finalSummary) return;
      dispatch({ type: 'SET_FINISHING', finishing: true });
      try {
        let summary: RoastSummary;
        if (st.mode === 'tauri') {
          summary = await ipc.finishSession({ roastUuid: st.roastUuid, dropWeightLb, notes });
        } else {
          const points = rebaseSamples(samplesRef.current, st.chargeSessionSec);
          summary = buildLiveSummary({
            roastUuid: st.roastUuid,
            startedWallMs: st.startedWallMs ?? Date.now(),
            meta: st.meta,
            markers: st.markers,
            points,
            dropWeightLb,
            notes,
          });
        }
        await stopSource();
        dispatch({ type: 'FINALIZED', summary });
      } catch (err) {
        dispatch({ type: 'SET_FINISHING', finishing: false });
        throw err;
      }
    },
    [stopSource],
  );

  const abandonRoast = useCallback(async () => {
    const st = stateRef.current;
    if (st.mode === 'tauri' && st.roastUuid) {
      try {
        await ipc.abandonSession({ roastUuid: st.roastUuid });
      } catch {
        /* best-effort */
      }
    }
    await stopSource();
    resetSamples();
    dispatch({ type: 'RESET' });
  }, [stopSource, resetSamples]);

  const startAnother = useCallback(async () => {
    await stopSource();
    resetSamples();
    dispatch({ type: 'RESET' });
  }, [stopSource, resetSamples]);

  const navigate = useCallback(
    (screen: SessionState['screen'], params?: NavParams) =>
      dispatch({ type: 'NAVIGATE', screen, params }),
    [],
  );

  const setReference = useCallback(
    (reference: ReferenceState | null) => dispatch({ type: 'SET_REFERENCE', reference }),
    [],
  );

  const actions = useMemo<SessionContextValue['actions']>(
    () => ({
      startRoast,
      resumeRoast,
      markEvent,
      markNext,
      undoEvent,
      undoLast,
      finishRoast,
      abandonRoast,
      navigate,
      startAnother,
      previewSummary,
      stopCaptureNow,
      setReference,
    }),
    [
      startRoast,
      resumeRoast,
      markEvent,
      markNext,
      undoEvent,
      undoLast,
      finishRoast,
      abandonRoast,
      navigate,
      startAnother,
      previewSummary,
      stopCaptureNow,
      setReference,
    ],
  );

  // Stable ref-readers: rAF consumers poll these without subscribing to renders.
  const getLatestArrival = useCallback((): SampleArrival | null => latestArrivalRef.current, []);
  const getSamples = useCallback((): readonly LiveSample[] => samplesRef.current, []);

  const value = useMemo<SessionContextValue>(
    () => ({ state, derived, capturing, getLatestArrival, getSamples, coolDown, actions }),
    [state, derived, capturing, getLatestArrival, getSamples, coolDown, actions],
  );

  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}

/** Fixtures available in browser demo mode, as selectable "sources". */
export function browserFixtureSources(): Array<{ key: string; fixture: RoastFixture }> {
  return Object.entries(BUNDLED_FIXTURES).map(([key, fixture]) => ({ key, fixture }));
}
