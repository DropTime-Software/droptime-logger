/**
 * Update-notice helpers (CONTRACTS §7.4). Notice-only in v0.1.0: the backend
 * `check_for_update` GETs the releases feed and reports whether a newer version
 * exists; the frontend surfaces a dismissible banner and opens the releases
 * page. Throttled so the launch check runs at most once a day.
 */
import { ipc, type AppMode, type UpdateInfo } from '../../bridge';
import { readSetting, writeSetting } from './settings';

/** Launch checks run at most once per this window (24h). */
export const UPDATE_THROTTLE_MS = 24 * 60 * 60 * 1000;

export function dismissedKey(version: string): string {
  return `updates.dismissed.${version}`;
}

/** Run the backend check (Tauri only). Best-effort — resolves `null` on any issue. */
export async function runUpdateCheck(mode: AppMode): Promise<UpdateInfo | null> {
  if (mode !== 'tauri') return null;
  try {
    const res = await ipc.checkForUpdate();
    await writeSetting(mode, 'updates.lastCheckMs', String(Date.now()));
    return res;
  } catch {
    return null;
  }
}

/** True when enough time has passed since the last recorded launch check. */
export async function launchCheckDue(mode: AppMode): Promise<boolean> {
  const raw = await readSetting(mode, 'updates.lastCheckMs');
  const last = raw ? Number(raw) : 0;
  if (!Number.isFinite(last) || last <= 0) return true;
  return Date.now() - last >= UPDATE_THROTTLE_MS;
}

export async function isVersionDismissed(mode: AppMode, version: string): Promise<boolean> {
  return (await readSetting(mode, dismissedKey(version))) === 'true';
}

export async function dismissVersion(mode: AppMode, version: string): Promise<void> {
  await writeSetting(mode, dismissedKey(version), 'true');
}
