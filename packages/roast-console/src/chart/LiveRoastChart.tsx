'use client';

/**
 * LiveRoastChart — the live-mode fork of droptime-app's proven bespoke SVG
 * roast chart (stretched 0..100 viewBox, non-scaling strokes, HTML overlay
 * labels so text never distorts, dual axis BT-left / RoR-right, phase
 * shading). What the fork adds for live capture:
 *
 *  - STABLE AXES: grow-only hysteresis domains while `live` — the time axis
 *    expands in 2-minute steps and the temp axis in 20 °F steps, never
 *    shrinking mid-roast, so there is no per-sample rescale jitter.
 *  - Leading-edge follow: x-domain = max(target span, elapsed + followWindow)
 *    so the curve never touches the right wall. `liveEdge.t` participates in
 *    the follow calculation BEFORE hysteresis, so the smoothed edge can never
 *    outrun the domain.
 *  - `liveEdge`: an interpolated leading-edge point (fed per animation frame
 *    by the consumer). The committed history stays untouched; the chart draws
 *    a short extension segment from the last committed point to the edge and
 *    parks the glowing pulse there — 60 fps motion, honest data.
 *  - Crosshair scrub on hover AND touch (touch freezes the readout pill);
 *    drag-to-zoom with double-click reset when NOT live.
 *
 * Pure SVG paths + HTML overlays; no chart library. The heavy history path
 * strings are memoized on [points identity, domains] so a per-frame liveEdge
 * update only recomputes the edge segment, dot and crosshair — the 1–2 Hz
 * append path holds 60 fps at ≤ 900 pts. Geometry is always °F/seconds; the
 * `unit` prop converts display text only.
 */

import { useId, useMemo, useRef, useState } from 'react';
import type { PointerEvent as ReactPointerEvent } from 'react';
import type { CurvePoint, RoastMarkersSec, TargetProfile } from '../types';
import { tokens } from '../tokens';
import { fmtTemp, mmss, tempSuffix, type TempUnit } from './format';
import { RcKeyframes } from './Keyframes';

export interface LiveEdge {
  /** seconds-from-charge of the interpolated leading edge */
  t: number;
  bt: number;
  et?: number;
}

export interface LiveRoastChartProps {
  points: CurvePoint[];
  markers: RoastMarkersSec;
  target?: TargetProfile | null;
  /** prior-roast comparison overlay */
  ghost?: CurvePoint[] | null;
  charged: boolean;
  /** right-edge breathing room ahead of the leading edge while live (default 120 s) */
  followWindowSec?: number;
  height?: number;
  showRoR?: boolean;
  showEt?: boolean;
  /** display unit for temperature captions/readouts (geometry stays °F); default 'F' */
  unit?: TempUnit;
  live?: boolean;
  /** smoothed leading-edge point (display-layer interpolation, live only) */
  liveEdge?: LiveEdge | null;
}

const X_STEP = 120; // grow the time axis in 2-minute steps
const BT_STEP = 20; // grow the temperature axis in 20 °F steps
const ROR_STEP = 10;

interface Domains {
  x0: number;
  x1: number;
  btMin: number;
  btMax: number;
  rorMin: number;
  rorMax: number;
}

function growMerge(prev: Domains, want: Domains): Domains {
  return {
    x0: Math.min(prev.x0, want.x0),
    x1: Math.max(prev.x1, want.x1),
    btMin: Math.min(prev.btMin, want.btMin),
    btMax: Math.max(prev.btMax, want.btMax),
    rorMin: Math.min(prev.rorMin, want.rorMin),
    rorMax: Math.max(prev.rorMax, want.rorMax),
  };
}

function timeTickStep(spanSec: number): number {
  for (const s of [15, 30, 60, 120, 180, 300, 600, 900]) {
    if (spanSec / s <= 8) return s;
  }
  return 1800;
}

export function LiveRoastChart({
  points,
  markers,
  target = null,
  ghost = null,
  charged,
  followWindowSec = 120,
  height = 320,
  showRoR = true,
  showEt = false,
  unit = 'F',
  live = false,
  liveEdge = null,
}: LiveRoastChartProps) {
  const clipId = useId();
  const plotRef = useRef<HTMLDivElement>(null);
  const growRef = useRef<Domains | null>(null);
  const chargedRef = useRef(charged);
  const [hover, setHover] = useState<{ t: number; frozen: boolean } | null>(null);
  const [sel, setSel] = useState<{ start: number; cur: number } | null>(null);
  const [zoom, setZoom] = useState<{ t0: number; t1: number } | null>(null);

  const targetCurve = target?.targetCurve ?? null;
  const hasGhost = !!ghost && ghost.length > 1;
  const hasTarget = !!targetCurve && targetCurve.length > 1;

  // ---------- domain inputs (memoized scan — stable across liveEdge frames) ----------

  const extent = useMemo(() => {
    const btVals: number[] = [];
    const rorVals: number[] = [];
    const tVals: number[] = [];
    for (const p of points) {
      tVals.push(p.t);
      btVals.push(p.bt);
      if (showEt && p.et !== undefined) btVals.push(p.et);
      if (p.ror !== undefined) rorVals.push(p.ror);
    }
    if (ghost) {
      for (const p of ghost) {
        tVals.push(p.t);
        btVals.push(p.bt);
      }
    }
    if (targetCurve) {
      for (const p of targetCurve) {
        tVals.push(p.t);
        btVals.push(p.bt);
        if (p.ror !== undefined) rorVals.push(p.ror);
      }
    }
    if (markers.chargeTempF !== undefined) btVals.push(markers.chargeTempF);
    return {
      minT: tVals.length ? Math.min(...tVals) : 0,
      maxT: tVals.length ? Math.max(...tVals) : 0,
      loBt: btVals.length ? Math.min(...btVals) : 150,
      hiBt: btVals.length ? Math.max(...btVals) : 450,
      loRor: rorVals.length ? Math.min(...rorVals) : 0,
      hiRor: rorVals.length ? Math.max(...rorVals) : 0,
    };
  }, [points, ghost, targetCurve, showEt, markers.chargeTempF]);

  // ---------- domains (grow-only hysteresis while live) ----------

  const lastPt = points.at(-1);
  // the smoothed edge participates in follow + temp extents BEFORE hysteresis
  const edge = live && liveEdge && lastPt && liveEdge.t >= lastPt.t ? liveEdge : null;
  const elapsed = Math.max(lastPt?.t ?? 0, edge?.t ?? -Infinity);
  const targetEnd = targetCurve?.at(-1)?.t ?? target?.targetDropTimeSec ?? 0;
  const ghostEnd = ghost?.at(-1)?.t ?? 0;

  let loBt = extent.loBt;
  let hiBt = extent.hiBt;
  if (edge) {
    loBt = Math.min(loBt, edge.bt, edge.et ?? edge.bt);
    hiBt = Math.max(hiBt, edge.bt, edge.et ?? edge.bt);
  }

  const want: Domains = {
    x0: live ? Math.min(0, Math.floor(extent.minT / 60) * 60) : Math.min(0, extent.minT),
    x1: live
      ? Math.ceil(Math.max(targetEnd, ghostEnd, elapsed + followWindowSec, X_STEP) / X_STEP) * X_STEP
      : Math.max(extent.maxT, 1),
    btMin: Math.floor((loBt - 10) / BT_STEP) * BT_STEP,
    btMax: Math.ceil((hiBt + 10) / BT_STEP) * BT_STEP,
    rorMin: Math.min(0, Math.floor((extent.loRor - 2) / ROR_STEP) * ROR_STEP),
    rorMax: Math.max(20, Math.ceil((extent.hiRor + 2) / ROR_STEP) * ROR_STEP),
  };

  // reset hysteresis on session boundaries: not-live, a fresh (empty) live
  // session, or the charge rebase shifting every t value under our feet
  if (chargedRef.current !== charged) {
    chargedRef.current = charged;
    growRef.current = null;
  }
  if (!live || points.length === 0) growRef.current = null;

  let dom = want;
  if (live) {
    const prev = growRef.current;
    dom = prev ? growMerge(prev, want) : want;
    growRef.current = dom; // idempotent (grow-only), safe under double-render
  }

  const dom0 = !live && zoom ? zoom.t0 : dom.x0;
  const dom1 = !live && zoom ? zoom.t1 : dom.x1;
  const domSpan = Math.max(1, dom1 - dom0);
  const btSpan = Math.max(1, dom.btMax - dom.btMin);
  const rorSpan = Math.max(1, dom.rorMax - dom.rorMin);

  const xOf = (t: number) => ((t - dom0) / domSpan) * 100;
  const tAtFrac = (frac: number) => dom0 + frac * domSpan;
  const yBt = (bt: number) => 100 - ((bt - dom.btMin) / btSpan) * 100;
  const yRor = (r: number) => 100 - ((r - dom.rorMin) / rorSpan) * 100;

  // ---------- heavy history paths (memoized: only points/domain changes rebuild;
  // per-frame liveEdge updates skip this entirely) ----------

  const { btPath, etPath, rorPath, ghostPath, targetPath } = useMemo(() => {
    const x = (t: number) => ((t - dom0) / domSpan) * 100;
    const yB = (bt: number) => 100 - ((bt - dom.btMin) / btSpan) * 100;
    const yR = (r: number) => 100 - ((r - dom.rorMin) / rorSpan) * 100;

    // pen-up on undefined values so gaps stay gaps
    const buildPath = <P extends { t: number }>(
      pts: readonly P[],
      val: (p: P) => number | undefined,
      y: (v: number) => number
    ): string => {
      let d = '';
      let pen = false;
      for (const p of pts) {
        const v = val(p);
        if (v === undefined) {
          pen = false;
          continue;
        }
        d += `${pen ? 'L' : 'M'}${x(p.t).toFixed(2)},${y(v).toFixed(2)} `;
        pen = true;
      }
      return d.trim();
    };

    return {
      btPath: buildPath(points, (p) => p.bt, yB),
      etPath: showEt ? buildPath(points, (p) => p.et, yB) : '',
      rorPath: showRoR ? buildPath(points, (p) => p.ror, yR) : '',
      ghostPath: hasGhost && ghost ? buildPath(ghost, (p) => p.bt, yB) : '',
      targetPath: hasTarget && targetCurve ? buildPath(targetCurve, (p) => p.bt, yB) : '',
    };
    // domains are primitives; scale fns above are pure functions of them
  }, [
    points,
    ghost,
    targetCurve,
    hasGhost,
    hasTarget,
    showEt,
    showRoR,
    dom0,
    domSpan,
    dom.btMin,
    btSpan,
    dom.rorMin,
    rorSpan,
  ]);

  if (points.length === 0 && !hasTarget && !hasGhost) {
    growRef.current = null; // fresh session: drop any grow-only domain history
    return (
      <div style={{ height }} className="flex items-center justify-center gap-2.5">
        <RcKeyframes />
        {live && (
          <span
            className="rc-anim inline-block h-2 w-2 rounded-full"
            style={{ background: tokens.forest, animation: 'rc-pulse 1.6s ease-in-out infinite' }}
          />
        )}
        <span className="text-sm" style={{ color: tokens.textDim }}>
          {live ? 'Waiting for first sample…' : 'No curve data'}
        </span>
      </div>
    );
  }

  // ---------- live-edge extension (recomputed per frame — cheap) ----------

  const edgeBtPath =
    edge && lastPt
      ? `M${xOf(lastPt.t).toFixed(2)},${yBt(lastPt.bt).toFixed(2)} L${xOf(edge.t).toFixed(2)},${yBt(edge.bt).toFixed(2)}`
      : '';
  const edgeEtPath =
    edge && lastPt && showEt && edge.et !== undefined && lastPt.et !== undefined
      ? `M${xOf(lastPt.t).toFixed(2)},${yBt(lastPt.et).toFixed(2)} L${xOf(edge.t).toFixed(2)},${yBt(edge.et).toFixed(2)}`
      : '';

  // ---------- markers → phase shading + event rules ----------

  const dryEnd = markers.dryEndSec;
  const fcStart = markers.fcStartSec;
  const devEnd = markers.dropSec ?? Math.min(elapsed, dom1);
  const shades: Array<{ key: string; a: number; b: number; fill: string; opacity: number }> = [];
  if (charged) {
    if (dryEnd !== undefined) shades.push({ key: 'dry', a: 0, b: dryEnd, fill: tokens.bandDrying, opacity: 0.05 });
    if (dryEnd !== undefined && fcStart !== undefined)
      shades.push({ key: 'mail', a: dryEnd, b: fcStart, fill: tokens.bandMaillard, opacity: 0.05 });
    if (fcStart !== undefined)
      shades.push({ key: 'dev', a: fcStart, b: devEnd, fill: tokens.bandDevelopment, opacity: 0.06 });
  }

  const rules: Array<{ key: string; label: string; t: number; color: string }> = [];
  if (charged) rules.push({ key: 'charge', label: 'CHARGE', t: 0, color: tokens.textDim });
  if (markers.turningPointSec !== undefined)
    rules.push({ key: 'tp', label: 'TP', t: markers.turningPointSec, color: tokens.textFaint });
  if (dryEnd !== undefined) rules.push({ key: 'dry', label: 'DRY', t: dryEnd, color: tokens.textDim });
  if (fcStart !== undefined) rules.push({ key: 'fc', label: 'FC', t: fcStart, color: tokens.fcMarker });
  if (markers.fcEndSec !== undefined)
    rules.push({ key: 'fce', label: 'FC END', t: markers.fcEndSec, color: tokens.fcMarker });
  if (markers.dropSec !== undefined)
    rules.push({ key: 'drop', label: 'DROP', t: markers.dropSec, color: tokens.forest });

  // ---------- ticks ----------

  const step = timeTickStep(domSpan);
  const timeTicks: number[] = [];
  for (let t = Math.ceil(dom0 / step) * step; t <= dom1 + 0.001; t += step) timeTicks.push(t);
  const tempTicks = [0, 1, 2, 3, 4].map((i) => dom.btMin + (btSpan * i) / 4);
  const rorTicks = [dom.rorMin, (dom.rorMin + dom.rorMax) / 2, dom.rorMax];

  // ---------- interaction ----------

  const tAtClientX = (clientX: number): number => {
    const el = plotRef.current;
    if (!el) return dom0;
    const r = el.getBoundingClientRect();
    return tAtFrac(Math.min(1, Math.max(0, (clientX - r.left) / Math.max(1, r.width))));
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    const t = tAtClientX(e.clientX);
    if (!live) setSel({ start: t, cur: t });
    setHover({ t, frozen: e.pointerType !== 'mouse' });
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const t = tAtClientX(e.clientX);
    setSel((s) => (s ? { ...s, cur: t } : s));
    setHover((h) => ({ t, frozen: h?.frozen ?? false }));
  };
  const onPointerUp = () => {
    if (!sel) return;
    const lo = Math.min(sel.start, sel.cur);
    const hi = Math.max(sel.start, sel.cur);
    setSel(null);
    if (!live && hi - lo > domSpan * 0.03) {
      setZoom({ t0: lo, t1: hi });
      setHover(null);
    }
  };
  const onPointerLeave = () => {
    setSel(null);
    setHover((h) => (h?.frozen ? h : null));
  };
  const onDoubleClick = () => {
    setZoom(null);
    setHover(null);
  };

  let hoverPt: CurvePoint | undefined;
  if (hover) {
    let bd = Infinity;
    for (const p of points) {
      const d = Math.abs(p.t - hover.t);
      if (d < bd) {
        bd = d;
        hoverPt = p;
      }
    }
  }
  const hoverInDomain = hoverPt !== undefined && hoverPt.t >= dom0 && hoverPt.t <= dom1;
  const hoverX = hoverPt ? xOf(hoverPt.t) : 0;

  // the glowing pulse rides the smoothed edge when present, else the last point
  const dotT = edge ? edge.t : lastPt?.t;
  const dotBt = edge ? edge.bt : lastPt?.bt;

  const PAD = { left: 48, right: showRoR ? 46 : 16, top: 18, bottom: 26 };
  const plotH = height - PAD.top - PAD.bottom;

  return (
    <div>
      <RcKeyframes />
      <div className="relative" style={{ height }}>
        {/* axis unit captions */}
        <div className="absolute left-0 top-0 text-[9px] font-medium" style={{ color: tokens.textFaint }}>
          {tempSuffix(unit)}
        </div>
        {showRoR && (
          <div className="absolute right-0 top-0 text-[9px] font-medium" style={{ color: tokens.ror, opacity: 0.8 }}>
            °F/min
          </div>
        )}

        {/* y-axis (temp) labels */}
        {tempTicks.map((v) => (
          <div
            key={`bt${v}`}
            className="absolute text-[10px] tabular-nums"
            style={{ left: 0, top: PAD.top + (yBt(v) / 100) * plotH - 6, color: tokens.textDim }}
          >
            {fmtTemp(v, unit)}°
          </div>
        ))}
        {/* y-axis (RoR) labels — always °F/min */}
        {showRoR &&
          rorTicks.map((v) => (
            <div
              key={`ror${v}`}
              className="absolute text-[10px] tabular-nums"
              style={{ right: 0, top: PAD.top + (yRor(v) / 100) * plotH - 6, color: tokens.ror, opacity: 0.85 }}
            >
              {v.toFixed(0)}
            </div>
          ))}
        {/* x-axis (time) labels */}
        {timeTicks.map((t) => (
          <div
            key={`x${t}`}
            className="absolute text-[10px] tabular-nums"
            style={{
              bottom: 4,
              left: `calc(${PAD.left}px + (100% - ${PAD.left + PAD.right}px) * ${xOf(t) / 100})`,
              transform: 'translateX(-50%)',
              color: tokens.textDim,
            }}
          >
            {mmss(t)}
          </div>
        ))}

        {/* zoom state pill (non-live only) */}
        {!live && zoom && (
          <button
            type="button"
            onClick={() => setZoom(null)}
            className="absolute -top-1 z-20 rounded-md px-2 py-0.5 text-[10px] font-semibold tabular-nums focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#024522]"
            style={{ right: PAD.right, background: tokens.lemon, color: tokens.forest }}
          >
            {mmss(dom0)}–{mmss(dom1)} · reset zoom
          </button>
        )}

        {/* plot area */}
        <div
          ref={plotRef}
          className="absolute cursor-crosshair select-none"
          style={{
            left: PAD.left,
            right: PAD.right,
            top: PAD.top,
            bottom: PAD.bottom,
            touchAction: 'pan-y',
          }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerLeave={onPointerLeave}
          onDoubleClick={onDoubleClick}
          title={live ? undefined : 'Drag to zoom the time axis · double-click to reset'}
        >
          <svg
            viewBox="0 0 100 100"
            preserveAspectRatio="none"
            className="h-full w-full overflow-visible"
            role="img"
            aria-label="Roast curve: bean temperature and rate of rise over time"
          >
            <defs>
              <clipPath id={clipId}>
                <rect x={0} y={0} width={100} height={100} />
              </clipPath>
            </defs>
            <g clipPath={`url(#${clipId})`}>
              {/* phase shading */}
              {shades.map((s) => (
                <rect
                  key={s.key}
                  x={xOf(s.a)}
                  y={0}
                  width={Math.max(0, xOf(s.b) - xOf(s.a))}
                  height={100}
                  fill={s.fill}
                  opacity={s.opacity}
                />
              ))}

              {/* gridlines */}
              {tempTicks.map((v) => (
                <line
                  key={`g${v}`}
                  x1={0}
                  y1={yBt(v)}
                  x2={100}
                  y2={yBt(v)}
                  stroke={tokens.gridline}
                  strokeWidth={1}
                  vectorEffect="non-scaling-stroke"
                />
              ))}
              {timeTicks.map((t) => (
                <line
                  key={`gv${t}`}
                  x1={xOf(t)}
                  y1={0}
                  x2={xOf(t)}
                  y2={100}
                  stroke={tokens.gridline}
                  strokeWidth={1}
                  vectorEffect="non-scaling-stroke"
                  opacity={0.6}
                />
              ))}

              {/* target ghost (dashed) */}
              {targetPath && (
                <path
                  d={targetPath}
                  fill="none"
                  stroke={tokens.target}
                  strokeWidth={1.5}
                  strokeDasharray="3 3"
                  vectorEffect="non-scaling-stroke"
                  opacity={0.75}
                />
              )}
              {/* prior-roast ghost */}
              {ghostPath && (
                <path
                  d={ghostPath}
                  fill="none"
                  stroke={tokens.ghost}
                  strokeWidth={1.3}
                  vectorEffect="non-scaling-stroke"
                  opacity={0.55}
                />
              )}
              {/* ET */}
              {etPath && (
                <path
                  d={etPath}
                  fill="none"
                  stroke={tokens.et}
                  strokeWidth={1.2}
                  strokeDasharray="1 2"
                  vectorEffect="non-scaling-stroke"
                  opacity={0.85}
                />
              )}
              {/* ET live-edge extension */}
              {edgeEtPath && (
                <path
                  d={edgeEtPath}
                  fill="none"
                  stroke={tokens.et}
                  strokeWidth={1.2}
                  strokeDasharray="1 2"
                  vectorEffect="non-scaling-stroke"
                  opacity={0.85}
                />
              )}
              {/* RoR (right axis, coral — reserved) */}
              {rorPath && (
                <path
                  d={rorPath}
                  fill="none"
                  stroke={tokens.ror}
                  strokeWidth={1.5}
                  vectorEffect="non-scaling-stroke"
                  opacity={0.9}
                />
              )}
              {/* BT (primary, forest) */}
              {btPath && (
                <path
                  d={btPath}
                  fill="none"
                  stroke={tokens.bt}
                  strokeWidth={2.2}
                  vectorEffect="non-scaling-stroke"
                  strokeLinejoin="round"
                />
              )}
              {/* BT live-edge extension — same stroke, so the tip reads as one line */}
              {edgeBtPath && (
                <path
                  d={edgeBtPath}
                  fill="none"
                  stroke={tokens.bt}
                  strokeWidth={2.2}
                  vectorEffect="non-scaling-stroke"
                  strokeLinecap="round"
                />
              )}

              {/* event vertical rules */}
              {rules.map((r) => (
                <line
                  key={r.key}
                  x1={xOf(r.t)}
                  y1={0}
                  x2={xOf(r.t)}
                  y2={100}
                  stroke={r.color}
                  strokeWidth={1}
                  strokeDasharray="2 2"
                  vectorEffect="non-scaling-stroke"
                  opacity={0.8}
                />
              ))}

              {/* drag-to-zoom selection */}
              {!live && sel && Math.abs(sel.cur - sel.start) > 0 && (
                <rect
                  x={xOf(Math.min(sel.start, sel.cur))}
                  y={0}
                  width={xOf(Math.max(sel.start, sel.cur)) - xOf(Math.min(sel.start, sel.cur))}
                  height={100}
                  fill={tokens.forest}
                  opacity={0.12}
                />
              )}

              {/* crosshair */}
              {hoverInDomain && (
                <line
                  x1={hoverX}
                  y1={0}
                  x2={hoverX}
                  y2={100}
                  stroke={tokens.pine}
                  strokeWidth={1}
                  vectorEffect="non-scaling-stroke"
                  opacity={0.5}
                />
              )}
            </g>
          </svg>

          {/* event labels (HTML overlay, undistorted) */}
          {rules.map((r) =>
            r.t < dom0 || r.t > dom1 ? null : (
              <div
                key={`lbl-${r.key}`}
                className="absolute -top-4 text-[9px] font-semibold tracking-tight whitespace-nowrap"
                style={{ left: `${xOf(r.t)}%`, transform: 'translateX(-50%)', color: r.color }}
              >
                {r.label}
              </div>
            )
          )}

          {/* live leading-edge pulse — rides the smoothed edge */}
          {live && dotT !== undefined && dotBt !== undefined && dotT >= dom0 && dotT <= dom1 && (
            <div
              className="pointer-events-none absolute z-10"
              style={{ left: `${xOf(dotT)}%`, top: `${yBt(dotBt)}%` }}
            >
              <span
                className="rc-anim absolute block h-3 w-3 rounded-full"
                style={{
                  background: tokens.forest,
                  transform: 'translate(-50%,-50%)',
                  animation: 'rc-ping 2.2s cubic-bezier(0,0,0.2,1) infinite',
                }}
              />
              <span
                className="absolute block h-2 w-2 rounded-full"
                style={{
                  background: tokens.forest,
                  transform: 'translate(-50%,-50%)',
                  boxShadow: '0 0 10px 1px rgba(2,69,34,0.45)',
                }}
              />
            </div>
          )}

          {/* crosshair marker dot on the BT curve */}
          {hoverInDomain && hoverPt && (
            <span
              className="pointer-events-none absolute block h-1.5 w-1.5 rounded-full"
              style={{
                left: `${hoverX}%`,
                top: `${yBt(hoverPt.bt)}%`,
                background: tokens.pine,
                transform: 'translate(-50%,-50%)',
              }}
            />
          )}

          {/* crosshair readout pill — dark ink pill, exactly like the app chart */}
          {hoverInDomain && hoverPt && (
            <div
              className="pointer-events-none absolute z-10 rounded-lg px-2.5 py-1.5 text-[11px] leading-4 shadow-lg tabular-nums"
              style={{
                left: `${hoverX}%`,
                top: 6,
                transform: hoverX > 70 ? 'translateX(-108%)' : 'translateX(10px)',
                background: tokens.tooltipBg,
                color: tokens.tooltipText,
              }}
            >
              <div className="font-semibold">{mmss(hoverPt.t)}</div>
              <div>BT {fmtTemp(hoverPt.bt, unit)}°</div>
              {hoverPt.ror !== undefined && (
                <div style={{ color: tokens.ror }}>RoR {hoverPt.ror.toFixed(1)}</div>
              )}
              {showEt && hoverPt.et !== undefined && (
                <div style={{ color: tokens.grass }}>ET {fmtTemp(hoverPt.et, unit)}°</div>
              )}
            </div>
          )}
        </div>
      </div>

      {/* legend — review mode only; live glanceability comes from BigNumbers */}
      {!live && (
        <div
          className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px]"
          style={{ paddingLeft: PAD.left, color: tokens.textDim }}
        >
          <LegendSwatch color={tokens.bt} label="BT" />
          {showRoR && <LegendSwatch color={tokens.ror} label="RoR (right)" />}
          {showEt && <LegendSwatch color={tokens.et} label="ET" dashed />}
          {ghostPath && <LegendSwatch color={tokens.ghost} label="Previous batch" />}
          {targetPath && <LegendSwatch color={tokens.target} label={target?.name ?? 'Target'} dashed />}
          {fcStart !== undefined && <LegendSwatch color={tokens.fcMarker} label="First crack" />}
        </div>
      )}
    </div>
  );
}

function LegendSwatch({ color, label, dashed }: { color: string; label: string; dashed?: boolean }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span
        className="inline-block h-0.5 w-4 rounded"
        style={{
          background: dashed
            ? `repeating-linear-gradient(90deg, ${color} 0 3px, transparent 3px 5px)`
            : color,
        }}
      />
      {label}
    </span>
  );
}
