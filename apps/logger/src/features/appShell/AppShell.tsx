/**
 * features/appShell — the native-app-feel layer (CONTRACTS §7.4 / §8, owner:
 * polisher). Mounted once from the always-present Header. It wires:
 *   - native menu events (`menu://about|check-updates|settings|shortcuts|report-issue`)
 *   - the `?` keyboard shortcut → shortcuts overlay
 *   - the throttled launch update check → dismissible banner
 *   - window-state persistence (restore on launch, save on move/resize)
 * All of it degrades cleanly to a no-op in the browser demo.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { ArrowUpRight, X } from '@phosphor-icons/react';
import { ipc, type AppInfo, type AppMode, type UpdateInfo } from '../../bridge';
import { useSession } from '../../state/SessionProvider';
import { useAlerts } from '../alerts/useAlerts';
import { AboutDialog } from './AboutDialog';
import { ShortcutsOverlay } from './ShortcutsOverlay';
import { openExternal } from './open';
import { ISSUES_URL } from './constants';
import { useWindowState } from './windowState';
import {
  dismissVersion,
  isVersionDismissed,
  launchCheckDue,
  runUpdateCheck,
} from './update';

const BROWSER_INFO: AppInfo = { version: '0.1.0', os: 'browser', arch: '' };

function isTextTarget(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  const tag = el.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable;
}

export function AppShell({ mode }: { mode: AppMode }) {
  const { actions } = useSession();

  const [appInfo, setAppInfo] = useState<AppInfo | null>(mode === 'tauri' ? null : BROWSER_INFO);
  const [showAbout, setShowAbout] = useState(false);
  const [showShortcuts, setShowShortcuts] = useState(false);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useWindowState(mode);
  // Alert rules engine lives here (always mounted) rather than in LiveScreen so
  // 'time after Drop' rules can still fire during the post-drop cool-down window —
  // marking drop navigates away from LiveScreen, which previously unmounted it.
  useAlerts();

  const flashToast = useCallback((message: string) => {
    setToast(message);
    if (toastTimer.current) clearTimeout(toastTimer.current);
    toastTimer.current = setTimeout(() => setToast(null), 3200);
  }, []);

  // App info (version/os/arch) once, for About + version display.
  useEffect(() => {
    if (mode !== 'tauri') return;
    let cancelled = false;
    ipc
      .getAppInfo()
      .then((info) => {
        if (!cancelled) setAppInfo(info);
      })
      .catch(() => {
        if (!cancelled) setAppInfo(BROWSER_INFO);
      });
    return () => {
      cancelled = true;
    };
  }, [mode]);

  // Manual "Check for Updates…" (menu + could be reused): show banner if newer,
  // otherwise a reassuring toast.
  const checkNow = useCallback(async () => {
    const res = await runUpdateCheck(mode);
    if (res?.isNewer) {
      setUpdate(res);
    } else if (res) {
      flashToast(`You're up to date — v${res.currentVersion}`);
    } else {
      flashToast('Could not check for updates right now.');
    }
  }, [mode, flashToast]);

  // Native menu events → app-shell behaviors (Tauri only).
  useEffect(() => {
    if (mode !== 'tauri') return;
    let cancelled = false;
    const unlisten: Array<() => void> = [];
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const wire = async (id: string, fn: () => void) => {
          const un = await listen(`menu://${id}`, () => fn());
          if (cancelled) un();
          else unlisten.push(un);
        };
        await wire('about', () => setShowAbout(true));
        await wire('shortcuts', () => setShowShortcuts(true));
        await wire('settings', () => actions.navigate('settings'));
        await wire('report-issue', () => void openExternal(mode, ISSUES_URL));
        await wire('check-updates', () => void checkNow());
      } catch {
        /* event API unavailable — menu still works, just unhandled */
      }
    })();
    return () => {
      cancelled = true;
      for (const u of unlisten) {
        try {
          u();
        } catch {
          /* ignore */
        }
      }
    };
  }, [mode, actions, checkNow]);

  // `?` opens the shortcuts overlay (ignored while typing / with modifiers).
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (isTextTarget(document.activeElement)) return;
      if (e.key === '?') {
        e.preventDefault();
        setShowShortcuts((s) => !s);
      }
    }
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // Throttled launch update check → banner unless this version was dismissed.
  useEffect(() => {
    if (mode !== 'tauri') return;
    let cancelled = false;
    void (async () => {
      if (!(await launchCheckDue(mode))) return;
      const res = await runUpdateCheck(mode);
      if (cancelled || !res?.isNewer) return;
      if (await isVersionDismissed(mode, res.latestVersion)) return;
      if (!cancelled) setUpdate(res);
    })();
    return () => {
      cancelled = true;
    };
  }, [mode]);

  const onDismissUpdate = useCallback(() => {
    if (update) void dismissVersion(mode, update.latestVersion);
    setUpdate(null);
  }, [mode, update]);

  return (
    <>
      {update ? (
        <div className="px-5 pt-2">
          <div
            className="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm"
            style={{ background: '#daf698', border: '1px solid rgba(2,69,34,0.25)', color: '#0d2418' }}
          >
            <button
              type="button"
              onClick={() => void openExternal(mode, update.url)}
              className="dt-focus-ring flex flex-1 items-center gap-1.5 text-left font-medium tracking-[-0.2px]"
            >
              Droptime Logger v{update.latestVersion} is available
              <ArrowUpRight size={15} weight="bold" />
            </button>
            <button
              type="button"
              aria-label="Dismiss update notice"
              onClick={onDismissUpdate}
              className="dt-focus-ring rounded-md p-0.5 text-forest/60 transition-colors hover:text-forest"
            >
              <X size={15} weight="bold" />
            </button>
          </div>
        </div>
      ) : null}

      {toast ? (
        <div className="pointer-events-none fixed inset-x-0 bottom-6 z-50 flex justify-center px-6">
          <div
            className="rounded-full bg-pine px-4 py-2 text-sm font-medium text-cream shadow-2xl"
            style={{ animation: 'dt-fade-in 0.18s ease' }}
            role="status"
          >
            {toast}
          </div>
        </div>
      ) : null}

      {showAbout ? (
        <AboutDialog info={appInfo} mode={mode} onClose={() => setShowAbout(false)} />
      ) : null}

      {showShortcuts ? <ShortcutsOverlay onClose={() => setShowShortcuts(false)} /> : null}
    </>
  );
}
