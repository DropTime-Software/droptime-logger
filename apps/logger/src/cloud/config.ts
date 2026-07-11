/// <reference types="vite/client" />
/**
 * Droptime Cloud configuration (build plan §9/§10).
 *
 * Only the LIVE publishable key ships in the desktop bundle (public by design);
 * the Clerk secret key stays server-side in droptime-app. The prod Convex URL
 * is NOT derivable from source — it must be injected at Vite build time as
 * VITE_CONVEX_URL (from the Convex dashboard for app.trydroptime.com). Until it
 * is set, Cloud sync reports "not configured" rather than failing at runtime.
 */

const env = import.meta.env as Record<string, string | undefined>;

/** LIVE Clerk publishable key → FAPI host clerk.trydroptime.com (public). */
export const CLERK_PUBLISHABLE_KEY =
  env.VITE_CLERK_PUBLISHABLE_KEY || 'pk_live_Y2xlcmsudHJ5ZHJvcHRpbWUuY29tJA';

/** Prod Convex deployment URL (*.convex.cloud). Build-time only; no default. */
export const CONVEX_URL = env.VITE_CONVEX_URL || '';

/** The droptime-app origin that mints the sign-in token (/api/logger/auth). */
export const APP_ORIGIN = env.VITE_APP_ORIGIN || 'https://app.trydroptime.com';

/** Clerk Frontend API host — the only host the cloud_fetch proxy may reach. */
export const CLERK_FAPI_ORIGIN = 'https://clerk.trydroptime.com';

/** Sign-in handoff endpoint. */
export const LOGGER_AUTH_URL = `${APP_ORIGIN}/api/logger/auth`;

/** Where onboarding (create org / choose Free plan) lives. */
export const APP_DASHBOARD_URL = `${APP_ORIGIN}/dashboard`;

/**
 * Cloud sync is available only when a Convex URL was baked in AND we have a
 * publishable key. Absent either, the UI shows a "not configured" state and the
 * flusher is inert (the logger stays fully functional, local-first).
 */
export function cloudConfigured(): boolean {
  return Boolean(CONVEX_URL) && Boolean(CLERK_PUBLISHABLE_KEY);
}
