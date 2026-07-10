/**
 * features/import — .alog / CSV import (CONTRACTS §7.1/§8, owner: importer).
 *
 * Two ways in: click-to-browse (dialog plugin, multiple, .alog/.csv filters) or
 * dragging files onto the window. Selected paths run through
 * `ipc.previewImport` (parse-only) into a per-file confirmation list; "Import"
 * commits the OK files via `ipc.importRoasts`, reports the count, and calls
 * `onImported` with the new roast uuids.
 *
 * Native OS drag-drop is the only path that yields real file *paths* in Tauri
 * (the webview intercepts HTML5 drops), so we listen for it via the webview
 * event rather than a DOM `ondrop`. Browser mode has no backend/persistence, so
 * this renders nothing.
 */
import { useEffect, useRef, useState } from 'react';
import { CheckCircle, UploadSimple, WarningCircle, X } from '@phosphor-icons/react';
import { open } from '@tauri-apps/plugin-dialog';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { mmss } from '@droptime/roast-console';
import { asLoggerError, ipc, isTauri, type ImportPreviewDto } from '../../bridge';
import { Button } from '../../components/ui';

const EXTENSIONS = ['alog', 'csv'];

function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

function hasImportExt(path: string): boolean {
  const dot = path.lastIndexOf('.');
  return dot >= 0 && EXTENSIONS.includes(path.slice(dot + 1).toLowerCase());
}

export interface ImportDropzoneProps {
  /** Called with the newly imported roast uuids after a successful commit. */
  onImported?: (roastUuids: string[]) => void;
}

export function ImportDropzone({ onImported }: ImportDropzoneProps) {
  const [previews, setPreviews] = useState<ImportPreviewDto[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [dragActive, setDragActive] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resultMsg, setResultMsg] = useState<string | null>(null);
  // Guards the drag-drop closure against re-entrant previews.
  const previewingRef = useRef(false);

  async function runPreview(paths: string[]) {
    const wanted = paths.filter(hasImportExt);
    if (wanted.length === 0) {
      setError('Drop .alog or .csv files to import.');
      return;
    }
    setError(null);
    setResultMsg(null);
    previewingRef.current = true;
    try {
      setPreviews(await ipc.previewImport({ paths: wanted }));
    } catch (err) {
      setError(asLoggerError(err).message);
    } finally {
      previewingRef.current = false;
    }
  }

  async function browse() {
    let selected: string | string[] | null;
    try {
      selected = await open({
        multiple: true,
        filters: [{ name: 'Artisan / CSV roast logs', extensions: EXTENSIONS }],
      });
    } catch (err) {
      setError(asLoggerError(err).message);
      return;
    }
    if (!selected) return;
    await runPreview(Array.isArray(selected) ? selected : [selected]);
  }

  async function importAll() {
    if (!previews) return;
    const paths = previews.filter((p) => p.ok).map((p) => p.path);
    if (paths.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      const result = await ipc.importRoasts({ paths });
      const n = result.imported;
      setResultMsg(`Imported ${n} ${n === 1 ? 'roast' : 'roasts'}.`);
      setPreviews(null);
      if (result.roastUuids.length > 0) onImported?.(result.roastUuids);
    } catch (err) {
      setError(asLoggerError(err).message);
    } finally {
      setBusy(false);
    }
  }

  // Native (OS-level) file drag-drop. HTML5 drops are swallowed by the webview,
  // so this is the only source of real file paths.
  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    let unlisten: (() => void) | undefined;
    void (async () => {
      const un = await getCurrentWebview().onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === 'enter') {
          if (p.paths.some(hasImportExt)) setDragActive(true);
        } else if (p.type === 'leave') {
          setDragActive(false);
        } else if (p.type === 'drop') {
          setDragActive(false);
          if (!previewingRef.current) void runPreview(p.paths);
        }
      });
      if (active) unlisten = un;
      else un();
    })();
    return () => {
      active = false;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Browser mode: no backend to import into.
  if (!isTauri()) return null;

  const okCount = previews?.filter((p) => p.ok).length ?? 0;

  return (
    <div className="flex w-full flex-col gap-3">
      {previews === null ? (
        <button
          type="button"
          onClick={() => void browse()}
          className={`dt-focus-ring flex w-full flex-col items-center gap-2 rounded-2xl border-2 border-dashed px-6 py-8 text-center transition ${
            dragActive
              ? 'border-pine bg-mint'
              : 'border-gray-200 bg-mint/30 hover:border-pine/40 hover:bg-mint/50'
          }`}
        >
          <UploadSimple size={26} weight="bold" className="text-pine/70" />
          <span className="text-sm font-semibold text-pine">
            {dragActive ? 'Drop to import' : 'Import roasts'}
          </span>
          <span className="text-xs text-ink/50">
            Drag Artisan <code>.alog</code> or CSV files here, or click to browse.
          </span>
        </button>
      ) : (
        <div className="dt-card overflow-hidden">
          <div className="flex items-center justify-between border-b border-gray-100 px-4 py-3">
            <span className="text-sm font-semibold text-pine">
              {previews.length} file{previews.length === 1 ? '' : 's'} · {okCount} ready
            </span>
            <button
              type="button"
              aria-label="Cancel import"
              className="dt-focus-ring text-ink/40 hover:text-ink/70"
              onClick={() => {
                setPreviews(null);
                setError(null);
              }}
              disabled={busy}
            >
              <X size={16} weight="bold" />
            </button>
          </div>
          <ul className="max-h-64 divide-y divide-gray-50 overflow-y-auto">
            {previews.map((p) => (
              <li key={p.path} className="flex items-center gap-3 px-4 py-2.5">
                {p.ok ? (
                  <CheckCircle size={18} weight="fill" className="shrink-0 text-pine" />
                ) : (
                  <WarningCircle size={18} weight="fill" className="shrink-0 text-red-500" />
                )}
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-pine">
                    {p.coffeeName?.trim() || basename(p.path)}
                  </p>
                  <p className="truncate text-xs text-ink/50">
                    {p.ok
                      ? [
                          p.sampleCount != null ? `${p.sampleCount} samples` : null,
                          p.durationSec != null ? mmss(p.durationSec) : null,
                          basename(p.path),
                        ]
                          .filter(Boolean)
                          .join(' · ')
                      : (p.error ?? 'Could not read this file.')}
                  </p>
                </div>
              </li>
            ))}
          </ul>
          <div className="flex items-center justify-end gap-2 border-t border-gray-100 px-4 py-3">
            <Button variant="ghost" onClick={() => setPreviews(null)} disabled={busy}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => void importAll()}
              disabled={busy || okCount === 0}
            >
              {busy ? 'Importing…' : okCount > 0 ? `Import ${okCount}` : 'Import'}
            </Button>
          </div>
        </div>
      )}

      {resultMsg ? (
        <p className="flex items-center gap-1.5 text-xs text-pine">
          <CheckCircle size={14} weight="fill" />
          {resultMsg}
        </p>
      ) : null}
      {error ? (
        <p className="flex items-center gap-1.5 text-xs text-red-600">
          <WarningCircle size={14} weight="fill" />
          {error}
        </p>
      ) : null}
    </div>
  );
}
