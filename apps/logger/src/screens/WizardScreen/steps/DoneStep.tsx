/**
 * Step 5 — You're all set. A short recap of what the wizard configured; the
 * shell's primary button ("Start roasting") persists `wizard.completed` and
 * lands on the setup screen.
 */
import { CheckCircle, PlugsConnected, UploadSimple, Thermometer, CoffeeBean } from '@phosphor-icons/react';
import type { AppMode } from '../../../bridge';
import type { WizardPath } from '../machine';

export function DoneStep({
  mode,
  path,
  importedCount,
  savedMachineName,
  unit,
}: {
  mode: AppMode;
  path: WizardPath;
  importedCount: number;
  savedMachineName?: string;
  unit: 'F' | 'C';
}) {
  const rows: Array<{ icon: React.ReactNode; text: React.ReactNode }> = [];

  if (savedMachineName) {
    rows.push({
      icon: <PlugsConnected size={18} weight="duotone" />,
      text: (
        <>
          Roaster <b className="text-pine">{savedMachineName}</b> saved and ready to log
        </>
      ),
    });
  } else if (mode === 'tauri' && path === 'demo') {
    rows.push({
      icon: <CoffeeBean size={18} weight="duotone" />,
      text: <>Exploring with bundled sample roasts — connect a roaster anytime from Settings</>,
    });
  }

  rows.push({
    icon: <UploadSimple size={18} weight="duotone" />,
    text:
      importedCount > 0 ? (
        <>
          <b className="text-pine">{importedCount}</b> roast{importedCount === 1 ? '' : 's'} imported
          into your history
        </>
      ) : (
        <>No history imported — you can bring roasts in later from the Roast history screen</>
      ),
  });

  rows.push({
    icon: <Thermometer size={18} weight="duotone" />,
    text: (
      <>
        Showing temperatures in <b className="text-pine">°{unit}</b>
      </>
    ),
  });

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center gap-3">
        <span className="flex h-12 w-12 items-center justify-center rounded-2xl bg-forest text-lemon">
          <CheckCircle size={26} weight="fill" />
        </span>
        <p className="text-sm leading-relaxed text-ink/60">
          That's everything. Your logger is ready — capture is local-first, so your roasts are
          saved on this machine the moment you hit start.
        </p>
      </div>

      <ul className="flex flex-col divide-y divide-line rounded-2xl border border-line bg-white">
        {rows.map((row, i) => (
          <li key={i} className="flex items-center gap-3 px-5 py-3.5 text-sm text-ink/70">
            <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-mint text-forest">
              {row.icon}
            </span>
            <span className="leading-relaxed">{row.text}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
