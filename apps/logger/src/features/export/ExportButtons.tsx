/**
 * features/export — .alog / CSV / JSON export actions for SummaryScreen +
 * DetailScreen (CONTRACTS §7.1/§8, owner: importer).
 *
 * Each button opens the native save dialog (dialog plugin) to choose a
 * destination, then calls `ipc.exportRoast`. A subtle inline line confirms the
 * write; errors surface readably. Renders nothing when `roastUuid` is undefined
 * or in browser mode (no backend / persistence to export from).
 */
import { useState } from 'react';
import { CheckCircle, DownloadSimple, WarningCircle } from '@phosphor-icons/react';
import { save } from '@tauri-apps/plugin-dialog';
import { asLoggerError, ipc, isTauri, type ExportFormat } from '../../bridge';
import { Button } from '../../components/ui';

interface FormatSpec {
  format: ExportFormat;
  label: string;
  ext: string;
  /** Dialog filter label. */
  name: string;
}

const FORMATS: FormatSpec[] = [
  { format: 'alog', label: '.alog', ext: 'alog', name: 'Artisan roast log' },
  { format: 'csv', label: 'CSV', ext: 'csv', name: 'CSV' },
  { format: 'json', label: 'JSON', ext: 'json', name: 'JSON' },
];

function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export interface ExportButtonsProps {
  roastUuid?: string;
}

export function ExportButtons({ roastUuid }: ExportButtonsProps) {
  const [busy, setBusy] = useState<ExportFormat | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function run({ format, ext, name }: FormatSpec) {
    if (!roastUuid || busy) return;
    setError(null);
    setSaved(null);
    let destPath: string | null;
    try {
      destPath = await save({
        defaultPath: `droptime-roast.${ext}`,
        filters: [{ name, extensions: [ext] }],
      });
    } catch (err) {
      setError(asLoggerError(err).message);
      return;
    }
    if (!destPath) return; // dialog cancelled
    setBusy(format);
    try {
      const { path } = await ipc.exportRoast({ roastUuid, format, destPath });
      setSaved(basename(path));
    } catch (err) {
      setError(asLoggerError(err).message);
    } finally {
      setBusy(null);
    }
  }

  // Nothing to export, or browser mode (no backend / persistence).
  if (!roastUuid || !isTauri()) return null;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xs font-semibold uppercase tracking-wide text-ink/45">Export</span>
        {FORMATS.map((spec) => (
          <Button
            key={spec.format}
            variant="ghost"
            onClick={() => void run(spec)}
            disabled={busy !== null}
          >
            <DownloadSimple size={15} weight="bold" />
            {busy === spec.format ? 'Saving…' : spec.label}
          </Button>
        ))}
      </div>
      {saved ? (
        <p className="flex items-center gap-1.5 text-xs text-pine">
          <CheckCircle size={14} weight="fill" />
          Exported to {saved}
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
