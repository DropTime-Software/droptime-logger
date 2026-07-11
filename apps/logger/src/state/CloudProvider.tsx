/**
 * CloudProvider — global Droptime Cloud auth + sync state (mirrors the small
 * SettingsProvider shape). Degrades to `disabled` in browser/demo mode and when
 * no Convex URL was baked in; the logger stays fully functional either way.
 *
 * Sync is opportunistic: on launch (if signed in), after sign-in, and on demand
 * via "Sync now". The pending count reads from the local outbox regardless of
 * auth so a signed-out user still sees "N roasts waiting to sync".
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react';

import { ipc } from '../bridge';
import { isTauri } from '../bridge/env';
import { cloudConfigured } from '../cloud/config';
import { flushOutbox, type Cap, type Gate } from '../cloud/flusher';
import { CloudSession, type CloudAuthState } from '../cloud/session';

export type CloudStatus = 'disabled' | 'signed-out' | 'signed-in';

export interface SyncStatus {
  pending: number;
  syncing: boolean;
  lastError?: string;
  gate?: Gate;
  cap?: Cap;
  lastSyncedAt?: number;
}

interface CloudContextValue {
  status: CloudStatus;
  auth: CloudAuthState;
  sync: SyncStatus;
  configured: boolean;
  busy: boolean;
  signIn: () => Promise<void>;
  signOut: () => Promise<void>;
  syncNow: () => Promise<void>;
}

const CloudContext = createContext<CloudContextValue | null>(null);

const DISABLED: CloudContextValue = {
  status: 'disabled',
  auth: { status: 'signed-out' },
  sync: { pending: 0, syncing: false },
  configured: false,
  busy: false,
  signIn: async () => {},
  signOut: async () => {},
  syncNow: async () => {},
};

export function CloudProvider({ children }: { children: ReactNode }) {
  const enabled = isTauri() && cloudConfigured();
  const sessionRef = useRef<CloudSession | null>(null);
  const [auth, setAuth] = useState<CloudAuthState>({ status: 'signed-out' });
  const [sync, setSync] = useState<SyncStatus>({ pending: 0, syncing: false });
  const [busy, setBusy] = useState(false);

  const refreshPending = useCallback(async () => {
    try {
      const pending = await ipc.syncPendingCount();
      setSync((s) => ({ ...s, pending }));
    } catch {
      /* local read; ignore */
    }
  }, []);

  const syncNow = useCallback(async () => {
    const session = sessionRef.current;
    if (!session || auth.status !== 'signed-in') return;
    const client = session.client();
    if (!client) return;
    setSync((s) => ({ ...s, syncing: true, lastError: undefined, gate: undefined, cap: undefined }));
    try {
      const result = await flushOutbox(client);
      setSync({
        pending: result.pending,
        syncing: false,
        lastError: result.error,
        gate: result.gate,
        cap: result.cap,
        lastSyncedAt: result.synced > 0 ? Date.now() : sync.lastSyncedAt,
      });
    } catch (err) {
      setSync((s) => ({
        ...s,
        syncing: false,
        lastError: err instanceof Error ? err.message : String(err),
      }));
    }
  }, [auth.status, sync.lastSyncedAt]);

  // Boot: load clerk-js (restoring any persisted session) + initial pending count.
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const session = new CloudSession();
    sessionRef.current = session;
    void refreshPending();
    session
      .init()
      .then((state) => {
        if (cancelled) return;
        setAuth(state);
        if (state.status === 'signed-in') void syncNow();
      })
      .catch((err) => {
        if (!cancelled) setSync((s) => ({ ...s, lastError: String(err) }));
      });
    return () => {
      cancelled = true;
    };
    // syncNow intentionally omitted: boot must run once, not on every status change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, refreshPending]);

  const signIn = useCallback(async () => {
    const session = sessionRef.current;
    if (!session) return;
    setBusy(true);
    try {
      const state = await session.signIn();
      setAuth(state);
      if (state.status === 'signed-in') await syncNow();
    } catch (err) {
      setSync((s) => ({ ...s, lastError: err instanceof Error ? err.message : String(err) }));
    } finally {
      setBusy(false);
    }
  }, [syncNow]);

  const signOut = useCallback(async () => {
    const session = sessionRef.current;
    if (!session) return;
    setBusy(true);
    try {
      setAuth(await session.signOut());
      await refreshPending();
    } finally {
      setBusy(false);
    }
  }, [refreshPending]);

  if (!enabled) return <CloudContext.Provider value={DISABLED}>{children}</CloudContext.Provider>;

  const value: CloudContextValue = {
    status: auth.status,
    auth,
    sync,
    configured: true,
    busy,
    signIn,
    signOut,
    syncNow,
  };
  return <CloudContext.Provider value={value}>{children}</CloudContext.Provider>;
}

export function useCloud(): CloudContextValue {
  return useContext(CloudContext) ?? DISABLED;
}
