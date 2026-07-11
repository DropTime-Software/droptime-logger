/**
 * Clerk FAPI fetch proxy (build plan §9; the technique tauri-plugin-clerk proves).
 *
 * A Tauri webview always injects an `Origin` header, and Clerk's FAPI rejects any
 * request carrying BOTH `Origin` and `Authorization` (clerk/javascript#4725). We
 * patch `window.fetch` so every request to clerk.trydroptime.com is forwarded
 * through the Rust `cloud_fetch` command, which omits `Origin` and keeps the
 * `Authorization`/`__client` token. All other fetches pass through untouched.
 *
 * GATE: this path can only be verified against the LIVE Clerk instance from an
 * installed build with a human sign-in; add the Tauri origins to the instance's
 * `allowed_origins` as belt-and-suspenders (see the PR handoff).
 */
import { invoke } from '@tauri-apps/api/core';

import { CLERK_FAPI_ORIGIN } from './config';

interface CloudFetchReq {
  method: string;
  headers: [string, string][];
  url: string;
  body?: string;
}
interface CloudFetchResp {
  status: number;
  headers: [string, string][];
  body: string;
}

let installed = false;

function urlOf(input: RequestInfo | URL): string {
  if (typeof input === 'string') return input;
  if (input instanceof URL) return input.href;
  return input.url;
}

function headersToPairs(h: HeadersInit | undefined): [string, string][] {
  if (!h) return [];
  if (h instanceof Headers) return [...h.entries()];
  if (Array.isArray(h)) return h.map(([k, v]) => [k, v]);
  return Object.entries(h);
}

async function bodyToString(
  input: RequestInfo | URL,
  init: RequestInit | undefined,
): Promise<string | undefined> {
  const body = init?.body;
  if (body == null) {
    // A Request object can itself carry the body.
    if (typeof input !== 'string' && !(input instanceof URL)) {
      try {
        return await input.clone().text();
      } catch {
        return undefined;
      }
    }
    return undefined;
  }
  if (typeof body === 'string') return body;
  if (body instanceof URLSearchParams) return body.toString();
  try {
    return await new Response(body as BodyInit).text();
  } catch {
    return undefined;
  }
}

/** Install the fetch proxy once, before `clerk.load()`. Idempotent. */
export function installClerkFetchProxy(): void {
  if (installed || typeof window === 'undefined') return;
  const original = window.fetch.bind(window);

  window.fetch = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = urlOf(input);
    if (!url.startsWith(CLERK_FAPI_ORIGIN)) return original(input, init);

    const reqMethod =
      init?.method ??
      (typeof input !== 'string' && !(input instanceof URL) ? input.method : undefined) ??
      'GET';
    const reqHeaders =
      headersToPairs(init?.headers) ||
      (typeof input !== 'string' && !(input instanceof URL)
        ? [...input.headers.entries()]
        : []);

    const req: CloudFetchReq = {
      method: reqMethod.toUpperCase(),
      url,
      headers: reqHeaders,
      body: await bodyToString(input, init),
    };
    const resp = await invoke<CloudFetchResp>('cloud_fetch', { req });
    return new Response(resp.body, {
      status: resp.status,
      headers: new Headers(resp.headers),
    });
  };

  installed = true;
}
