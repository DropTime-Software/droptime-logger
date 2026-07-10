/**
 * Open an external URL in the user's default browser. In Tauri this goes
 * through `tauri-plugin-opener` (the OS opener — no in-app navigation, so the
 * strict CSP is untouched); in the browser demo it falls back to `window.open`.
 */
import type { AppMode } from '../../bridge';

export async function openExternal(mode: AppMode, url: string): Promise<void> {
  if (mode === 'tauri') {
    try {
      const { openUrl } = await import('@tauri-apps/plugin-opener');
      await openUrl(url);
      return;
    } catch {
      /* fall through to window.open */
    }
  }
  try {
    window.open(url, '_blank', 'noopener,noreferrer');
  } catch {
    /* ignore */
  }
}
