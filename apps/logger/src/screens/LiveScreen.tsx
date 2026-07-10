import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import {
  BigNumbers,
  EventMarkerBar,
  LiveRoastChart,
  PhaseBars,
  type CurvePoint,
  type LiveEdge,
} from '@droptime/roast-console';
import { useSettings } from '../settings/SettingsProvider';
import { useSession } from '../state/SessionProvider';
import { KEY_TO_KIND } from '../state/roast';
import { ReferenceLayer } from '../features/reference/ReferenceLayer';

function isTextTarget(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  const tag = el.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable;
}

/** One display frame of the interpolated leading edge (presentation only). */
interface SmoothFrame {
  /** absolute session-seconds of the smoothed leading edge */
  sessionNow: number;
  bt: number;
  et?: number;
  ror?: number;
}

/** Exponential-smoothing time constant for displayed values (ms). */
const EASE_TAU_MS = 120;

/**
 * 60 fps display interpolation over the honest 1 Hz data. Each frame:
 *   - smoothSessionNow = latest.sessionSec + min(wallSinceArrival × speed,
 *     1.25 × observed inter-sample delta) — a stalled source freezes the edge
 *     instead of drawing fiction (status banners handle the rest);
 *   - bt/et slope-extrapolate from the last two raw samples, RoR from the last
 *     two RoR-bearing derived points;
 *   - the DISPLAYED values ease toward those targets (τ = 120 ms) so the
 *     re-anchor on every real sample never pops.
 * Data stays untouched: SQLite writes, sample cadence and marker math all read
 * the raw samples, never these frames.
 */
function useSmoothTelemetry(): SmoothFrame | null {
  const { state, derived, capturing, getLatestArrival, getSamples } = useSession();
  const [frame, setFrame] = useState<SmoothFrame | null>(null);

  const active = capturing && !state.finalSummary;
  const speed = state.meta?.speed ?? 1;

  // 1 Hz-derived context, readable inside the frame loop without restarting it.
  const loopCtx = useRef({ points: derived.points, ror: derived.ror, cs: state.chargeSessionSec });
  loopCtx.current = { points: derived.points, ror: derived.ror, cs: state.chargeSessionSec };

  const disp = useRef<{ bt?: number; et?: number; ror?: number; lastMs?: number }>({});

  useEffect(() => {
    if (!active) {
      disp.current = {};
      setFrame(null);
      return;
    }
    let raf = 0;
    const step = () => {
      raf = requestAnimationFrame(step);
      const arrival = getLatestArrival();
      if (!arrival) return;
      const latest = arrival.sample;
      const samples = getSamples();
      const prev = samples.length >= 2 ? samples[samples.length - 2] : undefined;

      // Observed inter-sample cadence in session-seconds (fallback 1 s).
      const deltaSess =
        prev && latest.sessionSec > prev.sessionSec ? latest.sessionSec - prev.sessionSec : 1;
      const maxAhead = 1.25 * deltaSess;
      const now = performance.now();
      const ahead = Math.min(Math.max(0, ((now - arrival.wallMs) / 1000) * speed), maxAhead);
      const sessionNow = latest.sessionSec + ahead;

      // Slope-extrapolated targets from the last two raw samples.
      const btSlope = prev ? (latest.btF - prev.btF) / deltaSess : 0;
      const btTarget = latest.btF + btSlope * ahead;
      let etTarget: number | undefined;
      if (latest.etF !== undefined) {
        const etSlope =
          prev && prev.etF !== undefined ? (latest.etF - prev.etF) / deltaSess : 0;
        etTarget = latest.etF + etSlope * ahead;
      }

      // RoR trend from the last two RoR-bearing derived points (1 Hz recompute).
      const { points, ror: derivedRor, cs } = loopCtx.current;
      let rorTarget = derivedRor;
      let p1: CurvePoint | undefined;
      let p0: CurvePoint | undefined;
      for (let i = points.length - 1; i >= 0; i--) {
        const p = points[i];
        if (p?.ror === undefined) continue;
        if (!p1) p1 = p;
        else {
          p0 = p;
          break;
        }
      }
      if (p1?.ror !== undefined && p0?.ror !== undefined && p1.t > p0.t) {
        const edgeT = sessionNow - (cs ?? 0);
        // Derived points lag the raw feed by ≤1 s; cap the horizon accordingly.
        const horizon = Math.min(Math.max(0, edgeT - p1.t), maxAhead + deltaSess);
        rorTarget = p1.ror + ((p1.ror - p0.ror) / (p1.t - p0.t)) * horizon;
      }

      // Ease displayed values toward targets so re-anchoring never pops.
      const lastMs = disp.current.lastMs ?? now;
      const dtMs = Math.min(250, Math.max(0, now - lastMs));
      const k = 1 - Math.exp(-dtMs / EASE_TAU_MS);
      const ease = (cur: number | undefined, target: number | undefined) =>
        target === undefined ? undefined : cur === undefined ? target : cur + (target - cur) * k;
      disp.current = {
        bt: ease(disp.current.bt, btTarget),
        et: ease(disp.current.et, etTarget),
        ror: ease(disp.current.ror, rorTarget),
        lastMs: now,
      };
      const bt = disp.current.bt;
      if (bt === undefined) return;
      setFrame({ sessionNow, bt, et: disp.current.et, ror: disp.current.ror });
    };
    raf = requestAnimationFrame(step);
    return () => {
      cancelAnimationFrame(raf);
      disp.current = {};
    };
  }, [active, speed, state.roastUuid, getLatestArrival, getSamples]);

  return frame;
}

export function LiveScreen() {
  const { unit } = useSettings();
  const { state, derived, actions } = useSession();
  const chartRef = useRef<HTMLDivElement>(null);
  const [chartHeight, setChartHeight] = useState(420);
  const [confirmAbandon, setConfirmAbandon] = useState(false);
  const smooth = useSmoothTelemetry();

  // Measure the chart container so the SVG fills the available space (>=60vh).
  useLayoutEffect(() => {
    const el = chartRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const h = entries[0]?.contentRect.height;
      if (h && h > 0) setChartHeight(Math.round(h));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Keyboard shortcuts — no-ops while a text field is focused.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (isTextTarget(document.activeElement)) return;
      const key = e.key.toLowerCase();
      if (key === ' ' || e.code === 'Space') {
        e.preventDefault();
        void actions.markNext();
      } else if (key === 'u') {
        e.preventDefault();
        void actions.undoLast();
      } else {
        const kind = KEY_TO_KIND[key];
        if (kind) {
          e.preventDefault();
          void actions.markEvent(kind);
        }
      }
    }
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [actions]);

  const controlsDisabled = state.finishing || !!state.finalSummary;

  // ---- smoothed display values (fall back to honest 1 Hz derived state) ----
  const cs = state.chargeSessionSec;
  const displaySample =
    smooth && derived.latestSample
      ? {
          ...derived.latestSample,
          btF: smooth.bt,
          ...(smooth.et !== undefined ? { etF: smooth.et } : {}),
        }
      : derived.latestSample;
  const displayRor = smooth?.ror ?? derived.ror;
  const displayElapsed = smooth && cs != null ? smooth.sessionNow - cs : derived.elapsedT;
  const liveEdge: LiveEdge | null =
    smooth && derived.points.length > 0
      ? {
          t: smooth.sessionNow - (cs ?? 0),
          bt: smooth.bt,
          ...(smooth.et !== undefined ? { et: smooth.et } : {}),
        }
      : null;

  return (
    <div className="flex h-full min-h-0 flex-col gap-3 px-6 py-4">
      {/* session line */}
      <div className="flex items-center justify-between text-sm">
        <div className="flex items-center gap-2 text-ink/60">
          <span className="font-semibold text-pine">
            {state.meta?.coffeeName || 'Untitled roast'}
          </span>
          {state.meta?.machineName ? (
            <span className="text-ink/45">· {state.meta.machineName}</span>
          ) : null}
          {state.meta && state.meta.speed !== 1 ? (
            <span className="rounded-full bg-gray-100 px-2 py-0.5 text-[10px] font-semibold text-ink/60">
              {state.meta.speed}× replay
            </span>
          ) : null}
        </div>
        {confirmAbandon ? (
          <div className="flex items-center gap-2 text-xs">
            <span className="text-ink/60">Discard this roast?</span>
            <button
              className="dt-focus-ring font-semibold text-red-600"
              onClick={() => void actions.abandonRoast()}
            >
              Discard
            </button>
            <button
              className="dt-focus-ring text-ink/50"
              onClick={() => setConfirmAbandon(false)}
            >
              Keep
            </button>
          </div>
        ) : (
          <button
            className="dt-focus-ring text-xs text-ink/40 hover:text-ink/70"
            onClick={() => setConfirmAbandon(true)}
          >
            Discard
          </button>
        )}
      </div>

      {/* big numbers band */}
      <BigNumbers
        sample={displaySample}
        ror={displayRor}
        elapsedT={displayElapsed}
        phase={derived.phase}
        projection={derived.projection}
        targetDeltaF={null}
        unit={unit}
      />

      {/* phase bars */}
      <PhaseBars phase={derived.phase} />

      {/* chart — center; flex-1 takes all remaining height so the marker bar
          below is NEVER pushed off-screen (a hard 60vh floor did exactly that
          on short windows). 240px is the absolute floor. */}
      <div ref={chartRef} className="min-h-0 flex-1" style={{ minHeight: 240 }}>
        <LiveRoastChart
          points={derived.points}
          markers={state.markers}
          target={state.target}
          ghost={state.reference?.curve ?? null}
          charged={derived.charged}
          unit={unit}
          height={chartHeight}
          showRoR
          showEt
          live
          liveEdge={liveEdge}
        />
      </div>

      {/* Reference cue layer (feature mount point — renders null until it lands). */}
      <ReferenceLayer />

      {/* event marker bar */}
      <div>
        <EventMarkerBar
          markers={state.markers}
          charged={derived.charged}
          onMark={(kind) => void actions.markEvent(kind)}
          onUndo={(kind) => void actions.undoEvent(kind)}
          disabled={controlsDisabled}
        />
        <p className="mt-1.5 text-center text-[11px] text-ink/40">
          Space — mark next · C charge · D dry end · F first crack · G FC end · X drop · U undo
        </p>
      </div>
    </div>
  );
}
