/**
 * Runtime detection. Per CONTRACTS.md §4 the webview is running inside Tauri
 * when the internals bridge has been injected; otherwise we are in a plain
 * browser (dev / demo) and run a pure in-process simulator with no persistence.
 */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export type AppMode = 'tauri' | 'browser';

export function appMode(): AppMode {
  return isTauri() ? 'tauri' : 'browser';
}
