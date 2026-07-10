/**
 * OS notifications for alerts. Tauri routes through `tauri-plugin-notification`
 * (permission requested lazily on first use); the browser demo falls back to
 * the Web Notifications API. All calls are best-effort and never throw.
 */
import type { AppMode } from '../../bridge';

/** Request permission once and cache the grant so we don't re-prompt per alert. */
let tauriGrant: boolean | null = null;

async function ensureTauriPermission(): Promise<boolean> {
  if (tauriGrant !== null) return tauriGrant;
  try {
    const { isPermissionGranted, requestPermission } = await import(
      '@tauri-apps/plugin-notification'
    );
    let granted = await isPermissionGranted();
    if (!granted) granted = (await requestPermission()) === 'granted';
    tauriGrant = granted;
    return granted;
  } catch {
    tauriGrant = false;
    return false;
  }
}

async function notifyTauri(title: string, body: string): Promise<void> {
  if (!(await ensureTauriPermission())) return;
  try {
    const { sendNotification } = await import('@tauri-apps/plugin-notification');
    sendNotification({ title, body });
  } catch {
    /* ignore */
  }
}

async function notifyBrowser(title: string, body: string): Promise<void> {
  try {
    if (typeof window === 'undefined' || !('Notification' in window)) return;
    let perm = Notification.permission;
    if (perm === 'default') perm = await Notification.requestPermission();
    if (perm === 'granted') new Notification(title, { body });
  } catch {
    /* ignore */
  }
}

export async function notify(mode: AppMode, title: string, body: string): Promise<void> {
  if (mode === 'tauri') return notifyTauri(title, body);
  return notifyBrowser(title, body);
}
