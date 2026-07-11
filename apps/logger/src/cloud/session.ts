/**
 * CloudSession — owns clerk-js (headless) + a Convex HTTP client for the flusher.
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
 * The flusher POSTs mutations through a `ConvexHttpClient` with a fresh
 * `convex`-template Clerk JWT set per roast — the HTTP client authenticates each
 * request directly, with none of the WebSocket client's connection-auth timing.
 *
 * GATE: end-to-end auth needs the LIVE Clerk instance + a human sign-in from an
 * installed build; it cannot be exercised headlessly.
 */
import { Clerk } from '@clerk/clerk-js';
import { ConvexHttpClient } from 'convex/browser';
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
  private convex: ConvexHttpClient | null = null;
  private ready = false;

  /** Load clerk-js (restoring any persisted session) and create the client. */
  async init(): Promise<CloudAuthState> {
    if (this.ready) return this.state();
    installClerkFetchProxy();
    this.clerk = new Clerk(CLERK_PUBLISHABLE_KEY);
    // Native mode: clerk-js authenticates FAPI with an Authorization header +
    // in-memory client token instead of cookies. The Tauri webview can't carry
    // clerk.trydroptime.com cookies through the Rust fetch-proxy, so standard
    // (cookie) mode leaves getToken() unauthenticated → null. (build plan §9)
    await this.clerk.load({ standardBrowser: false });
    this.convex = new ConvexHttpClient(CONVEX_URL);
    this.ready = true;
    return this.state();
  }

  /** The HTTP client the flusher POSTs mutations through (null until init). */
  client(): ConvexHttpClient | null {
    return this.convex;
  }

  /**
   * A fresh `convex`-template JWT for the current session, or null if there's no
   * session. Lets a getToken() failure (e.g. missing JWT template) propagate so
   * the flusher can surface the real reason instead of a generic "expired".
   */
  async getToken(): Promise<string | null> {
    return (await this.clerk?.session?.getToken({ template: 'convex' })) ?? null;
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
    return this.state();
  }

  async signOut(): Promise<CloudAuthState> {
    await this.clerk?.signOut();
    this.convex?.clearAuth();
    return this.state();
  }
}
