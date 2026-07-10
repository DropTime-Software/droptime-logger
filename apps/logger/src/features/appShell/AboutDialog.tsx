/**
 * About dialog — a small branded card with version / OS / arch and a couple of
 * quiet links. Reached from the native menu (`menu://about`) and Settings.
 * Styled to match RecoveryDialog (fixed overlay, white rounded card).
 */
import type { AppInfo, AppMode } from '../../bridge';
import { Button } from '../../components/ui';
import { openExternal } from './open';
import { ISSUES_URL, RELEASES_URL } from './constants';

export function AboutDialog({
  info,
  mode,
  onClose,
}: {
  info: AppInfo | null;
  mode: AppMode;
  onClose: () => void;
}) {
  const version = info?.version ?? '0.1.0';
  const platform = info ? `${prettyOs(info.os)} · ${info.arch}` : '—';

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 px-6"
      onClick={onClose}
    >
      <div
        className="w-full max-w-sm rounded-3xl bg-white p-6 text-center shadow-2xl"
        style={{ animation: 'dt-fade-in 0.2s ease' }}
        role="dialog"
        aria-modal="true"
        aria-labelledby="about-title"
        onClick={(e) => e.stopPropagation()}
      >
        <span className="mx-auto flex h-14 w-14 items-center justify-center rounded-2xl bg-[#f5eeba] text-2xl font-bold text-[#5a7a32]">
          D
        </span>
        <h2 id="about-title" className="mt-4 text-lg font-bold tracking-[-0.04em] text-pine">
          Droptime Logger<span className="text-grass">.</span>
        </h2>
        <p className="mt-1 text-sm text-ink/55">Free, local-first live roast logging.</p>

        <dl className="mt-5 space-y-1.5 rounded-xl bg-gray-50 p-3 text-sm">
          <Row label="Version" value={version} />
          <Row label="Platform" value={platform} />
          <Row label="License" value="AGPL-3.0 — open source" />
        </dl>

        <div className="mt-5 flex flex-wrap justify-center gap-2">
          <Button variant="ghost" onClick={() => void openExternal(mode, RELEASES_URL)}>
            Releases
          </Button>
          <Button variant="ghost" onClick={() => void openExternal(mode, ISSUES_URL)}>
            Report an issue
          </Button>
          <Button variant="primary" onClick={onClose}>
            Close
          </Button>
        </div>
      </div>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-4">
      <dt className="text-ink/50">{label}</dt>
      <dd className="font-medium tabular-nums text-pine">{value}</dd>
    </div>
  );
}

function prettyOs(os: string): string {
  switch (os) {
    case 'macos':
      return 'macOS';
    case 'windows':
      return 'Windows';
    case 'linux':
      return 'Linux';
    default:
      return os.charAt(0).toUpperCase() + os.slice(1);
  }
}
