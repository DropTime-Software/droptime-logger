/**
 * Step 4 — Bring your roast history. Mounts the importer's ImportDropzone
 * (frozen props contract, owner: importer) and records how many roasts landed.
 * The dropzone renders nothing until the real importer ships; in browser demo
 * mode there is no local store, so we explain that importing is a desktop
 * feature. This step is always optional — the shell lets the operator skip it.
 */
import { UploadSimple, CheckCircle } from '@phosphor-icons/react';
import type { AppMode } from '../../../bridge';
import { ImportDropzone } from '../../../features/import/ImportDropzone';

export function ImportStep({
  mode,
  importedCount,
  onImported,
}: {
  mode: AppMode;
  importedCount: number;
  onImported: (roastUuids: string[]) => void;
}) {
  return (
    <div className="flex flex-col gap-5">
      <p className="text-sm leading-relaxed text-ink/60">
        Already roasting with Artisan or another logger? Bring those roasts in so your history
        starts full — drop in <code className="rounded bg-mint px-1 py-0.5 text-xs text-forest">.alog</code>{' '}
        or CSV files. You can always import more later.
      </p>

      <ImportDropzone onImported={onImported} />

      {mode === 'browser' ? (
        <div className="flex items-start gap-3 rounded-2xl border border-dashed border-line2 bg-white px-5 py-6">
          <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-mint text-forest">
            <UploadSimple size={20} weight="duotone" />
          </span>
          <div>
            <div className="text-sm font-semibold text-pine">Import runs in the desktop app</div>
            <p className="mt-1 text-xs leading-relaxed text-ink/55">
              The browser demo doesn't keep a roast library. Install Droptime Logger to import
              your history and log for real — it stays free and local-first.
            </p>
          </div>
        </div>
      ) : null}

      {importedCount > 0 ? (
        <div className="flex items-center gap-2 rounded-xl border border-[rgba(60,140,90,0.4)] bg-[rgba(60,140,90,0.06)] px-4 py-3 text-sm font-medium text-grass">
          <CheckCircle size={17} weight="fill" />
          Imported {importedCount} roast{importedCount === 1 ? '' : 's'}. Nice — they're in your history.
        </div>
      ) : null}
    </div>
  );
}
