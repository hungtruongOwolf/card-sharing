import type { components } from '@card-sharing/api-client';

import { problemCode } from '../../data/problemCode';
import { SERVER_BUSY } from '../../data/serverBusy';
import { AuthError, type AuthClient } from '../auth';
import { createPkcePair, platformRandom, SecureRandomUnavailableError } from '../auth/pkce';
import * as SecureStore from '../../platform/secureStore';

type EmailStartRequest = components['schemas']['EmailStartRequest'];

/**
 * Email magic-link sign-in (T-22/T-23, ADR-0012).
 *
 * `start` asks the server for a link with a fresh PKCE challenge and keeps the verifier on this device under the
 * request id the server answers. The emailed link (`/auth/link#rid=<request id>&token=<token>`) usually opens in a new
 * browser tab, which finds the verifier by that id (the store is the device's secure store: `localStorage` on the
 * web) and redeems the link with it. The record also carries the invite code and the link the user started from,
 * so the new tab can use the code and continue there. Records live for the link's 15 minutes (a stale one reports
 * "expired"), are deleted after use or a final refusal, and at most a handful are kept.
 *
 * The server is the authority: it refuses another device's verifier, a used or expired link and a missing or wrong
 * invite code; this module only maps those answers to what the screens say.
 */

/** Lifetime of a link on the server (crates/identity/src/magic_link.rs). */
export const LINK_TTL_MS = 15 * 60 * 1000;
/** Records older than this are dropped even if never opened. */
const RECORD_RETENTION_MS = 60 * 60 * 1000;
const MAX_RECORDS = 5;

/** SecureStore keys allow only alphanumerics, `.`, `-` and `_`. Bump the version on a format change. */
export const MAGIC_LINK_INDEX_KEY = 'cs.magiclink.index.v1';
export const magicLinkKey = (requestId: string) => `cs.magiclink.v1.${requestId}`;

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const TOKEN = /^[A-Za-z0-9_-]{43}$/;
const VERIFIER = /^[A-Za-z0-9._~-]{43,128}$/;
/** Loose shape check before asking the server, which validates for real. */
const EMAIL = /^[^\s@]{1,64}@[^\s@]+\.[^\s@]+$/;
const INVITE_MAX = 32;

/** The key-value subset of the secure store this module uses (injectable for tests). */
export interface MagicLinkKeyValue {
  getItem(key: string): Promise<string | null>;
  setItem(key: string, value: string): Promise<void>;
  deleteItem(key: string): Promise<void>;
}

const SECURE_OPTIONS: SecureStore.SecureStoreOptions = {
  keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
};

/** The device's secure store (Keychain/Keystore; `localStorage` on the web, ADR-0009). */
export function secureMagicLinkStore(): MagicLinkKeyValue {
  return {
    getItem: (key) => SecureStore.getItemAsync(key, SECURE_OPTIONS),
    setItem: (key, value) => SecureStore.setItemAsync(key, value, SECURE_OPTIONS),
    deleteItem: (key) => SecureStore.deleteItemAsync(key, SECURE_OPTIONS),
  };
}

/** What the emailed link carries back (`#rid=<request id>&token=<token>`). */
export interface LinkFragment {
  readonly requestId: string;
  readonly token: string;
}

/** Reads `#rid=...&token=...`; null when either is missing or malformed. */
export function parseLinkFragment(hash: string): LinkFragment | null {
  // Parsed by hand: React Native's URLSearchParams polyfill does not implement every method on all versions.
  const params = new Map<string, string>();
  for (const pair of (hash.startsWith('#') ? hash.slice(1) : hash).split('&')) {
    const at = pair.indexOf('=');
    if (at > 0) params.set(pair.slice(0, at), pair.slice(at + 1));
  }
  const requestId = params.get('rid') ?? '';
  const token = params.get('token') ?? '';
  return UUID.test(requestId) && TOKEN.test(token) ? { requestId, token } : null;
}

/** What this device keeps for a link it asked for. */
export interface PendingMagicLink {
  readonly verifier: string;
  /** Epoch milliseconds. */
  readonly createdAt: number;
  /** Sign-up invite code to send with the link (from an invite link or typed). */
  readonly inviteCode?: string;
  /** The in-app link the user started from (an invite, a card), to open once signed in. */
  readonly returnTo?: string;
}

export type LinkLookup =
  | { readonly status: 'found'; readonly link: PendingMagicLink }
  /** This device asked for it more than 15 minutes ago. */
  | { readonly status: 'expired' }
  /** Not asked for on this device and browser (or already used). */
  | { readonly status: 'missing' };

export type MagicLinkStartErrorKind =
  /** 400: the server does not accept the address. */
  | 'invalid_email'
  /** 429: too many links for this address or network (AC-1.2). */
  | 'rate_limited'
  /** 503 `SERVER_BUSY`. */
  | 'busy'
  /** Network failure, 5xx or an unexpected body. */
  | 'unavailable'
  /** No secure random source on this platform: email sign-in is unavailable. */
  | 'not_supported';

export class MagicLinkStartError extends Error {
  override name = 'MagicLinkStartError';
  constructor(readonly kind: MagicLinkStartErrorKind) {
    super(`magic link start: ${kind}`);
  }
}

/** How redeeming a link ended. */
export type LinkOutcome =
  | 'signed_in'
  /** The link would create an account and the sign-up gate needs a code (the link is still usable). */
  | 'invite_required'
  /** The code is wrong, expired, revoked or used up (the link is still usable, up to the server's cap). */
  | 'invite_invalid'
  | 'expired'
  | 'used'
  /** Not this device's link, or unknown to the server. */
  | 'invalid'
  | 'suspended'
  | 'rate_limited'
  | 'unavailable';

export interface MagicLinkSignIn {
  /** Asks for a link to `email`; keeps the verifier (and `context`) under the request id it returns. */
  start(
    email: string,
    context?: { inviteCode?: string; returnTo?: string },
  ): Promise<{ requestId: string }>;
  /** The record of a link opened on this device. */
  lookup(requestId: string): Promise<LinkLookup>;
  /**
   * Redeems a link with the kept verifier. On `signed_in` the session is stored like any sign-in. The record is
   * kept while the link can still be used (invite needed or wrong, transient failures) and deleted otherwise.
   */
  complete(requestId: string, token: string, inviteCode?: string): Promise<LinkOutcome>;
}

export function isPlausibleEmail(email: string): boolean {
  return email.length <= 320 && EMAIL.test(email);
}

function parseRecord(raw: string | null): PendingMagicLink | null {
  if (raw === null) return null;
  try {
    const v = JSON.parse(raw) as Partial<Record<keyof PendingMagicLink, unknown>>;
    if (typeof v.verifier !== 'string' || !VERIFIER.test(v.verifier)) return null;
    if (typeof v.createdAt !== 'number' || !Number.isFinite(v.createdAt)) return null;
    const inviteCode =
      typeof v.inviteCode === 'string' && v.inviteCode.length <= INVITE_MAX
        ? v.inviteCode
        : undefined;
    const returnTo = typeof v.returnTo === 'string' ? v.returnTo : undefined;
    return { verifier: v.verifier, createdAt: v.createdAt, inviteCode, returnTo };
  } catch {
    return null;
  }
}

function parseIndex(raw: string | null): { id: string; at: number }[] {
  if (raw === null) return [];
  try {
    const value: unknown = JSON.parse(raw);
    if (!Array.isArray(value)) return [];
    return value.flatMap((item: unknown) => {
      if (typeof item !== 'object' || item === null) return [];
      const { id, at } = item as { id?: unknown; at?: unknown };
      return typeof id === 'string' && UUID.test(id) && typeof at === 'number' ? [{ id, at }] : [];
    });
  } catch {
    return [];
  }
}

export interface MagicLinkSignInOptions {
  baseUrl: () => string;
  authClient: Pick<AuthClient, 'signInWithMagicLink'>;
  store?: MagicLinkKeyValue;
  fetch?: typeof fetch;
  now?: () => number;
  random?: ((bytes: Uint8Array) => Uint8Array) | null;
}

export function createMagicLinkSignIn({
  baseUrl,
  authClient,
  store = secureMagicLinkStore(),
  fetch: fetchFn = (input, init) => fetch(input, init),
  now = Date.now,
  random = platformRandom(),
}: MagicLinkSignInOptions): MagicLinkSignIn {
  async function remove(requestId: string): Promise<void> {
    await store.deleteItem(magicLinkKey(requestId)).catch(() => undefined);
    const index = parseIndex(await store.getItem(MAGIC_LINK_INDEX_KEY).catch(() => null));
    const kept = index.filter((entry) => entry.id !== requestId);
    if (kept.length !== index.length) {
      await store.setItem(MAGIC_LINK_INDEX_KEY, JSON.stringify(kept)).catch(() => undefined);
    }
  }

  async function remember(requestId: string, link: PendingMagicLink): Promise<void> {
    const index = parseIndex(await store.getItem(MAGIC_LINK_INDEX_KEY).catch(() => null));
    const fresh = index.filter((entry) => now() - entry.at < RECORD_RETENTION_MS);
    const kept = [{ id: requestId, at: link.createdAt }, ...fresh].slice(0, MAX_RECORDS);
    for (const entry of index) {
      if (!kept.some((k) => k.id === entry.id)) {
        await store.deleteItem(magicLinkKey(entry.id)).catch(() => undefined);
      }
    }
    await store.setItem(magicLinkKey(requestId), JSON.stringify(link));
    await store.setItem(MAGIC_LINK_INDEX_KEY, JSON.stringify(kept));
  }

  async function lookup(requestId: string): Promise<LinkLookup> {
    if (!UUID.test(requestId)) return { status: 'missing' };
    const link = parseRecord(await store.getItem(magicLinkKey(requestId)).catch(() => null));
    if (!link) return { status: 'missing' };
    if (now() - link.createdAt >= LINK_TTL_MS) {
      await remove(requestId);
      return { status: 'expired' };
    }
    return { status: 'found', link };
  }

  return {
    async start(email, context = {}) {
      let pair;
      try {
        pair = createPkcePair(random);
      } catch (error) {
        if (error instanceof SecureRandomUnavailableError) {
          throw new MagicLinkStartError('not_supported');
        }
        throw error;
      }
      const body: EmailStartRequest = { email, code_challenge: pair.challenge };
      let response: Response;
      try {
        response = await fetchFn(`${baseUrl()}/v1/auth/email/start`, {
          method: 'POST',
          headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
      } catch {
        throw new MagicLinkStartError('unavailable');
      }
      if (response.status === 400) throw new MagicLinkStartError('invalid_email');
      if (response.status === 429) throw new MagicLinkStartError('rate_limited');
      if (response.status === 503 && (await problemCode(response)) === SERVER_BUSY) {
        throw new MagicLinkStartError('busy');
      }
      if (response.status !== 202) throw new MagicLinkStartError('unavailable');
      const answer: unknown = await response.json().catch(() => null);
      const requestId =
        typeof answer === 'object' && answer !== null && 'request_id' in answer
          ? (answer as { request_id: unknown }).request_id
          : null;
      if (typeof requestId !== 'string' || !UUID.test(requestId)) {
        throw new MagicLinkStartError('unavailable');
      }
      const inviteCode = context.inviteCode?.trim().slice(0, INVITE_MAX) || undefined;
      await remember(requestId, {
        verifier: pair.verifier,
        createdAt: now(),
        inviteCode,
        returnTo: context.returnTo,
      });
      return { requestId };
    },

    lookup,

    async complete(requestId, token, inviteCode) {
      const found = await lookup(requestId);
      if (found.status === 'expired') return 'expired';
      if (found.status === 'missing') return 'invalid';
      const code = inviteCode?.trim() || found.link.inviteCode;
      try {
        await authClient.signInWithMagicLink(token, found.link.verifier, {
          inviteCode: code ? code.slice(0, INVITE_MAX) : undefined,
        });
      } catch (error) {
        if (!(error instanceof AuthError)) return 'unavailable';
        if (error.status === 429) return 'rate_limited';
        if (error.kind === 'unavailable') return 'unavailable';
        switch (error.code) {
          case 'INVITE_REQUIRED':
            return 'invite_required';
          case 'INVITE_INVALID':
            return 'invite_invalid';
          case 'MAGIC_LINK_EXPIRED':
            await remove(requestId);
            return 'expired';
          case 'MAGIC_LINK_USED':
            await remove(requestId);
            return 'used';
          case 'ACCOUNT_SUSPENDED':
            await remove(requestId);
            return 'suspended';
          default:
            await remove(requestId);
            return 'invalid';
        }
      }
      await remove(requestId);
      return 'signed_in';
    },
  };
}
