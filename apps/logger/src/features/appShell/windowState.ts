/**
 * Window-bounds persistence (CONTRACTS §7.4 / §8 `window.state`). Restores the
 * saved size + position on launch, and saves them (debounced) on move/resize.
 * NEVER restores an off-screen window: a saved position is only re-applied when
 * it still lands on a connected monitor, otherwise the OS default (centered)
 * stands. Tauri-only; a no-op in the browser demo.
 */
import { useEffect } from 'react';
import type { AppMode } from '../../bridge';
import { readSetting, writeSetting } from './settings';

interface WindowBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

const SAVE_DEBOUNCE_MS = 400;
/** A saved position must expose at least this many px on a monitor to restore. */
const VISIBLE_MARGIN = 80;

function isBounds(v: unknown): v is WindowBounds {
  if (!v || typeof v !== 'object') return false;
  const b = v as Record<string, unknown>;
  return (
    typeof b.x === 'number' &&
    typeof b.y === 'number' &&
    typeof b.width === 'number' &&
    typeof b.height === 'number' &&
    b.width > 0 &&
    b.height > 0 &&
    Number.isFinite(b.x) &&
    Number.isFinite(b.y)
  );
}

interface MonitorRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** True when (x,y) leaves a decent chunk of the titlebar reachable on a screen. */
function positionOnScreen(x: number, y: number, width: number, monitors: MonitorRect[]): boolean {
  for (const m of monitors) {
    const overlapX = Math.min(x + width, m.x + m.w) - Math.max(x, m.x);
    const withinY = y >= m.y - 1 && y <= m.y + m.h - VISIBLE_MARGIN;
    if (overlapX >= VISIBLE_MARGIN && withinY) return true;
  }
  return false;
}

export function useWindowState(mode: AppMode): void {
  useEffect(() => {
    if (mode !== 'tauri') return;
    let disposed = false;
    let saveTimer: ReturnType<typeof setTimeout> | null = null;
    const unlisten: Array<() => void> = [];

    async function run() {
      try {
        const [{ getCurrentWindow, availableMonitors }, { PhysicalPosition, PhysicalSize }] =
          await Promise.all([
            import('@tauri-apps/api/window'),
            import('@tauri-apps/api/dpi'),
          ]);
        const win = getCurrentWindow();

        const monitors: MonitorRect[] = (await availableMonitors()).map((m) => ({
          x: m.position.x,
          y: m.position.y,
          w: m.size.width,
          h: m.size.height,
        }));

        // ---- restore ----
        const raw = await readSetting(mode, 'window.state');
        if (raw) {
          let saved: unknown = null;
          try {
            saved = JSON.parse(raw);
          } catch {
            /* corrupt value — ignore, keep OS default */
          }
          if (isBounds(saved) && !disposed) {
            try {
              await win.setSize(new PhysicalSize(saved.width, saved.height));
              if (positionOnScreen(saved.x, saved.y, saved.width, monitors)) {
                await win.setPosition(new PhysicalPosition(saved.x, saved.y));
              }
            } catch (err) {
              // Don't mistake an ACL denial for a corrupt value: surface it.
              console.warn(
                'window-state restore failed — check core:window set-size/set-position permissions',
                err,
              );
            }
          }
        }

        // ---- persist (debounced) ----
        const save = () => {
          if (saveTimer) clearTimeout(saveTimer);
          saveTimer = setTimeout(() => {
            void (async () => {
              try {
                if (disposed) return;
                if ((await win.isMinimized()) || (await win.isMaximized())) return;
                const pos = await win.outerPosition();
                const size = await win.outerSize();
                const bounds: WindowBounds = {
                  x: pos.x,
                  y: pos.y,
                  width: size.width,
                  height: size.height,
                };
                await writeSetting(mode, 'window.state', JSON.stringify(bounds));
              } catch {
                /* ignore transient window errors */
              }
            })();
          }, SAVE_DEBOUNCE_MS);
        };

        unlisten.push(await win.onMoved(save));
        unlisten.push(await win.onResized(save));
      } catch {
        /* window API unavailable — silently skip persistence */
      }
    }

    void run();

    return () => {
      disposed = true;
      if (saveTimer) clearTimeout(saveTimer);
      for (const u of unlisten) {
        try {
          u();
        } catch {
          /* ignore */
        }
      }
    };
  }, [mode]);
}
