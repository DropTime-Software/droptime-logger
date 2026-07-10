import { useState } from 'react';
import { mmss } from '@droptime/roast-console';
import { ipc, type RecoveryDto } from '../bridge';
import { useSession } from '../state/SessionProvider';
import { Button } from './ui';

export function RecoveryDialog({
  recovery,
  onClose,
}: {
  recovery: RecoveryDto;
  onClose: () => void;
}) {
  const { actions } = useSession();
  const [busy, setBusy] = useState<'resume' | 'discard' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const startedAt = new Date(recovery.startedWallMs).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  });

  async function onResume() {
    setBusy('resume');
    setError(null);
    try {
      await actions.resumeRoast(recovery);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not resume the roast.');
      setBusy(null);
    }
  }

  async function onDiscard() {
    setBusy('discard');
    setError(null);
    try {
      await ipc.discardRecovery({ roastUuid: recovery.roastUuid });
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not close the roast.');
      setBusy(null);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 px-6">
      <div
        className="w-full max-w-md rounded-3xl bg-white p-6 shadow-2xl"
        style={{ animation: 'dt-fade-in 0.2s ease' }}
        role="dialog"
        aria-modal="true"
        aria-labelledby="recovery-title"
      >
        <h2 id="recovery-title" className="text-lg font-bold tracking-[-0.04em] text-pine">
          Unfinished roast found
        </h2>
        <p className="mt-2 text-sm leading-relaxed text-ink/60">
          The app closed while a roast was still recording. Its data is safe. Pick up where
          you left off, or close it out and keep what was captured.
        </p>

        <dl className="mt-4 space-y-1.5 rounded-xl bg-gray-50 p-3 text-sm">
          <Row label="Coffee" value={recovery.meta.coffeeName ?? '—'} />
          <Row label="Started" value={startedAt} />
          <Row label="Captured" value={`${mmss(recovery.lastSessionSec)} · ${recovery.lastSeq} samples`} />
        </dl>

        {error ? <p className="mt-3 text-sm text-red-600">{error}</p> : null}

        <div className="mt-5 flex justify-end gap-2.5">
          <Button variant="ghost" onClick={onDiscard} disabled={busy !== null}>
            {busy === 'discard' ? 'Closing…' : 'Close it out'}
          </Button>
          <Button variant="primary" onClick={onResume} disabled={busy !== null}>
            {busy === 'resume' ? 'Resuming…' : 'Resume roast'}
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
      <dd className="font-medium text-pine">{value}</dd>
    </div>
  );
}
