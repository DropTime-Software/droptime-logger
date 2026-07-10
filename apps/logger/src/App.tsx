import { useEffect, useState } from 'react';
import { appMode, ipc, type AppMode, type RecoveryDto } from './bridge';
import { SessionProvider, useSession } from './state/SessionProvider';
import { SettingsProvider } from './settings/SettingsProvider';
import { Header } from './components/Header';
import { StatusBanners } from './components/StatusBanners';
import { RecoveryDialog } from './components/RecoveryDialog';
import { SetupScreen } from './screens/SetupScreen';
import { LiveScreen } from './screens/LiveScreen';
import { SummaryScreen } from './screens/SummaryScreen';
import { HistoryScreen } from './screens/HistoryScreen';
import { DetailScreen } from './screens/DetailScreen';
import { SettingsScreen } from './screens/SettingsScreen';
import { WizardScreen } from './screens/WizardScreen';

const MODE: AppMode = appMode();

export default function App() {
  return (
    <SettingsProvider mode={MODE}>
      <SessionProvider mode={MODE}>
        <AppShell mode={MODE} />
      </SessionProvider>
    </SettingsProvider>
  );
}

function AppShell({ mode }: { mode: AppMode }) {
  const { state, actions } = useSession();

  // undefined = still checking; null = nothing to recover.
  const [recovery, setRecovery] = useState<RecoveryDto | null | undefined>(
    mode === 'tauri' ? undefined : null,
  );

  useEffect(() => {
    if (mode !== 'tauri') return;
    let cancelled = false;
    ipc
      .pendingRecovery()
      .then((rec) => {
        if (!cancelled) setRecovery(rec);
      })
      .catch(() => {
        if (!cancelled) setRecovery(null);
      });
    return () => {
      cancelled = true;
    };
  }, [mode]);

  // First-run: send new installs to the setup wizard once.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const done =
          mode === 'tauri'
            ? await ipc.getSetting({ key: 'wizard.completed' })
            : localStorage.getItem('droptime.wizard.completed');
        if (!cancelled && done !== 'true') actions.navigate('wizard');
      } catch {
        // On error, don't force the wizard.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [mode, actions]);

  // Shell parity with the web app's AppShell: dark-pine chrome with the
  // content floating on a white rounded card that never scrolls the chrome.
  return (
    <div className="flex h-full flex-col bg-pine">
      <Header mode={mode} />
      <StatusBanners />
      <main className="min-h-0 flex-1 p-4 pt-2">
        <div className="flex h-full w-full flex-col overflow-hidden rounded-3xl bg-white shadow-2xl">
          {state.screen === 'setup' && <SetupScreen mode={mode} />}
          {state.screen === 'live' && <LiveScreen />}
          {state.screen === 'summary' && <SummaryScreen mode={mode} />}
          {state.screen === 'history' && <HistoryScreen mode={mode} />}
          {state.screen === 'detail' && <DetailScreen mode={mode} />}
          {state.screen === 'settings' && <SettingsScreen mode={mode} />}
          {state.screen === 'wizard' && <WizardScreen mode={mode} />}
        </div>
      </main>

      {recovery ? (
        <RecoveryDialog recovery={recovery} onClose={() => setRecovery(null)} />
      ) : null}
    </div>
  );
}
