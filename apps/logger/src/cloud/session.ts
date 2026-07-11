/**
 * CloudSession — owns clerk-js (headless) + the authed Convex client.
 *
 * Sign-in (build plan §9, loopback variant promoted to v1 primary):
 *   1. start a one-shot Rust loopback listener (oauth_start → port)
 *   2. open the system browser to /api/logger/auth?state=<nonce>&redirect_uri=loopback
 *   3. Clerk sign-in in the browser → the route mints a 60s sign-in token and
 *      302s back to the loopback → Rust emits `oauth-callback` {token, state}
 *   4. verify the nonce, then consume the ticket via ClerkJS native mode:
 *      signIn.create({ strategy: 'ticket', ticket }) → setActive  (pin this exact
 *      call; the signIn.ticket() wrapper is broken — clerk/javascript#8219)
 *
 * The Convex client authenticates with the Clerk `convex`-template JWT; clerk-js
 * owns the ~60s refresh loop.
 *
 * GATE: end-to-end auth needs the LIVE Clerk instance (allowed_origins) + a human
 * sign-in from an installed build; it cannot be exercised headlessly.
 */
import { Clerk } from '@clerk/clerk-js';
import { ConvexClient } from 'convex/browser';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { openUrl } from '@tauri-apps/plugin-opener';

import type { OAuthCallback } from '../bridge/dto';
import { CLERK_PUBLISHABLE_KEY, CONVEX_URL, LOGGER_AUTH_URL } from './config';
import { installClerkFetchProxy } from './clerkProxy';

export interface CloudAuthState {
  status: 'signed-out' | 'signed-in';
  email?: string;
  orgName?: string;
}

const SIGN_IN_TIMEOUT_MS = 5 * 60_000;

export class CloudSession {
  private clerk: Clerk | null = null;
  private convex: ConvexClient | null = null;
  private ready = false;

  /** Load clerk-js (restoring any persisted session) and wire the Convex auth. */
  async init(): Promise<CloudAuthState> {
    if (this.ready) return this.state();
    installClerkFetchProxy();
    this.clerk = new Clerk(CLERK_PUBLISHABLE_KEY);
    await this.clerk.load({});
    this.convex = new ConvexClient(CONVEX_URL);
    this.wireConvexAuth();
    this.ready = true;
    return this.state();
  }

  /** Point the Convex client at the current Clerk `convex`-template token. */
  private wireConvexAuth(): void {
    const clerk = this.clerk;
    this.convex?.setAuth(async ({ forceRefreshToken }: { forceRefreshToken: boolean }) => {
      try {
        return (
          (await clerk?.session?.getToken({ template: 'convex', skipCache: forceRefreshToken })) ??
          null
        );
      } catch {
        return null;
      }
    });
  }

  /** The authed Convex client the flusher drains through (null until init). */
  client(): ConvexClient | null {
    return this.convex;
  }

  state(): CloudAuthState {
    const clerk = this.clerk;
    if (!clerk?.session || !clerk.user) return { status: 'signed-out' };
    return {
      status: 'signed-in',
      email: clerk.user.primaryEmailAddress?.emailAddress,
      orgName: clerk.organization?.name,
    };
  }

  /** Run the browser → loopback → ticket handoff. Resolves to the new state. */
  async signIn(): Promise<CloudAuthState> {
    const clerk = this.clerk;
    if (!clerk) throw new Error('cloud session not initialized');

    const nonce = crypto.randomUUID();
    const port = await invoke<number>('oauth_start');

    const callback = new Promise<OAuthCallback>((resolve, reject) => {
      const timer = setTimeout(() => {
        void unlisten.then((fn) => fn());
        reject(new Error('sign-in timed out'));
      }, SIGN_IN_TIMEOUT_MS);
      const unlisten = listen<OAuthCallback>('oauth-callback', (event) => {
        clearTimeout(timer);
        void unlisten.then((fn) => fn());
        resolve(event.payload);
      });
    });

    const url = new URL(LOGGER_AUTH_URL);
    url.searchParams.set('state', nonce);
    url.searchParams.set('redirect_uri', `http://127.0.0.1:${port}/callback`);
    await openUrl(url.href);

    const { token, state } = await callback;
    if (state !== nonce) throw new Error('sign-in state mismatch');

    // Ticket create silently no-ops if a session is already active (#8044).
    if (clerk.session) await clerk.signOut();
    const client = clerk.client;
    if (!client) throw new Error('clerk client not loaded');
    const si = await client.signIn.create({ strategy: 'ticket', ticket: token });
    if (si.status !== 'complete' || !si.createdSessionId) {
      throw new Error(`sign-in incomplete (${si.status})`);
    }
    await clerk.setActive({ session: si.createdSessionId });
    this.wireConvexAuth();
    return this.state();
  }

  async signOut(): Promise<CloudAuthState> {
    await this.clerk?.signOut();
    this.wireConvexAuth();
    return this.state();
  }
}
