import type { AppMode } from '../bridge';
import type { Banner, SessionAction, SessionState } from './types';

export function initialState(mode: AppMode): SessionState {
  return {
    screen: 'setup',
    mode,
    markers: {},
    markHistory: [],
    banners: [],
    connection: 'idle',
    target: null,
    reference: null,
    finishing: false,
  };
}

function upsertBanner(banners: Banner[], banner: Banner): Banner[] {
  return [...banners.filter((b) => b.id !== banner.id), banner];
}

function removeBanner(banners: Banner[], id: string): Banner[] {
  return banners.filter((b) => b.id !== id);
}

/**
 * Reconcile status banners. Informational (non-error) tone throughout, per the
 * brief: gap / flatline / disconnected surface visible banners; connected /
 * reconnected clear the recovery banners.
 */
function reconcileBanners(
  banners: Banner[],
  kind: SessionState['connection'],
  message?: string,
): Banner[] {
  switch (kind) {
    case 'gap':
      return upsertBanner(banners, {
        id: 'gap',
        tone: 'warn',
        text: message ?? 'Signal gap — a sample was dropped',
      });
    case 'flatline':
      return upsertBanner(banners, {
        id: 'flatline',
        tone: 'warn',
        text: message ?? 'Probe reading is flat — check the probe connection',
      });
    case 'disconnected':
      return upsertBanner(banners, {
        id: 'disconnected',
        tone: 'warn',
        text: message ?? 'Source disconnected — attempting to recover',
      });
    case 'reconnected':
      return upsertBanner(removeBanner(banners, 'disconnected'), {
        id: 'reconnected',
        tone: 'good',
        text: message ?? 'Reconnected',
      });
    case 'connected':
      return removeBanner(removeBanner(banners, 'disconnected'), 'flatline');
    case 'ended':
      return upsertBanner(banners, {
        id: 'ended',
        tone: 'neutral',
        text: message ?? 'Replay ended — mark drop or finish the roast',
      });
    default:
      return banners;
  }
}

export function sessionReducer(state: SessionState, action: SessionAction): SessionState {
  switch (action.type) {
    case 'RESET':
      return initialState(state.mode);

    case 'SESSION_STARTED':
      return {
        ...initialState(state.mode),
        screen: 'live',
        roastUuid: action.roastUuid,
        startedWallMs: action.startedWallMs,
        meta: action.meta,
        connection: 'connected',
        // The reference is picked on Setup FOR the session being started —
        // starting must not clear it (only RESET does).
        reference: state.reference,
      };

    case 'RESUMED':
      return {
        ...initialState(state.mode),
        screen: 'live',
        roastUuid: action.roastUuid,
        startedWallMs: action.startedWallMs,
        meta: action.meta,
        markers: action.markers,
        chargeSessionSec: action.chargeSessionSec,
        markHistory: action.markHistory,
        connection: 'reconnected',
        reference: state.reference,
      };

    case 'APPLY_MARKERS':
      return {
        ...state,
        markers: action.markers,
        chargeSessionSec: action.chargeSessionSec,
        markHistory: action.markHistory,
      };

    case 'STATUS':
      return {
        ...state,
        connection: action.kind,
        banners: reconcileBanners(state.banners, action.kind, action.message),
      };

    case 'DISMISS_BANNER':
      return { ...state, banners: removeBanner(state.banners, action.id) };

    case 'PUSH_BANNER':
      return { ...state, banners: upsertBanner(state.banners, action.banner) };

    case 'NAVIGATE':
      return { ...state, screen: action.screen, navParams: action.params };

    case 'SET_REFERENCE':
      return { ...state, reference: action.reference };

    case 'SET_FINISHING':
      return { ...state, finishing: action.finishing };

    case 'FINALIZED':
      return {
        ...state,
        finalSummary: action.summary,
        finishing: false,
        markers: {
          turningPointSec: action.summary.turningPointSec,
          turningPointTempF: action.summary.turningPointTempF,
          dryEndSec: action.summary.dryEndSec,
          fcStartSec: action.summary.fcStartSec,
          fcEndSec: action.summary.fcEndSec,
          dropSec: action.summary.dropSec,
          dropTempF: action.summary.dropTempF,
          chargeTempF: action.summary.chargeTempF,
        },
      };

    default:
      return state;
  }
}
