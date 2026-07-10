/**
 * Droptime design tokens for the console — LIGHT, matching the Droptime web
 * app (app.trydroptime.com). The console renders on the app's floating white
 * card; hierarchy comes from scale and the brand greens, coral stays reserved
 * for RoR.
 */
export const tokens = {
  // brand
  forest: '#024522',
  pine: '#0D2418',
  lemon: '#DAF698',
  cream: '#FDF7EE',
  grass: '#3C8C5A',
  coral: '#fe6e51',
  // chart series (parity with droptime-app RoastCurveChart / charts/constants.ts)
  bt: '#024522',
  et: '#3C8C5A',
  ror: '#fe6e51',
  ghost: '#9ca3af',
  target: '#c4b5fd',
  fcMarker: '#f59e0b',
  // console surfaces (white card ground; hierarchy via scale, not boxes)
  surface: '#ffffff',
  hairline: 'rgba(36,36,36,0.12)',
  gridline: '#e5e7eb', // app chart gridline gray
  text: '#0D2418',
  textDim: 'rgba(36,36,36,0.55)',
  textFaint: 'rgba(36,36,36,0.35)',
  // dark ink readout pill — identical to the app chart's crosshair readout
  tooltipBg: '#0D2418',
  tooltipText: '#FDF7EE',
  // phase identities (chips/strips) — legible on white; coral stays RoR-only
  phaseDrying: '#d97706',
  phaseMaillard: '#B4551D',
  phaseDevelopment: '#024522',
  // phase shading bands in the chart — exact app-chart tints
  bandDrying: '#f59e0b',
  bandMaillard: '#fe6e51',
  bandDevelopment: '#024522',
  // status accents (deltas, callouts)
  good: '#3C8C5A',
  warn: '#f59e0b',
} as const;

export type Tokens = typeof tokens;

/**
 * The previous dim-roastery dark set, kept available for future dark-mode work.
 * Nothing in the console renders from this today — the console is light-only.
 */
export const darkTokens = {
  forest: '#024522',
  pine: '#0D2418',
  lemon: '#DAF698',
  cream: '#FDF7EE',
  grass: '#3C8C5A',
  coral: '#fe6e51',
  bt: '#DAF698',
  et: '#3C8C5A',
  ror: '#fe6e51',
  ghost: '#9ca3af',
  target: '#c4b5fd',
  fcMarker: '#f59e0b',
  surface: '#0D2418',
  pineRaised: '#143323',
  hairline: 'rgba(253,247,238,0.08)',
  gridline: 'rgba(253,247,238,0.07)',
  text: '#FDF7EE',
  textDim: 'rgba(253,247,238,0.45)',
  textFaint: 'rgba(253,247,238,0.28)',
  phaseDrying: '#f59e0b',
  phaseMaillard: '#C97B4A',
  phaseDevelopment: '#DAF698',
  good: '#3C8C5A',
  warn: '#f59e0b',
} as const;
