// Shared roast console core. Module ownership (Phase 1 build):
//   types.ts     — THE contract (frozen; changes require touching every consumer)
//   math/        — curve math: trailing RoR, phases, projections, TP detection (agent A)
//   simulator/   — SimulatorSource + fixture loading (agent A)
//   chart/       — LiveRoastChart, the live-mode SVG chart (agent B)
//   console/     — BigNumbers, PhaseBars, EventMarkerBar, RoastSummaryCard (agent B)
//   tokens.ts    — design tokens (agent B)
//   replay/      — background-replay reference cue math (v0.1.0, reference feature)

export * from './types';
export * from './math';
export * from './simulator';
export * from './chart';
export * from './console';
export * from './tokens';
export * from './replay';
