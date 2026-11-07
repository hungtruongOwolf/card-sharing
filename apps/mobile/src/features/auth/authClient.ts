import type { components } from '@card-sharing/api-client';

import { problemCode } from '../../data/problemCode';
import { sessionFromTokenPair, type Session } from './session';
import type { SessionSync } from './sessionSync';
import type { TokenStore } from './tokenStore';

type RefreshRequest = components['schemas']['RefreshRequest'];
type DevSignInRequest = components['schemas']['DevSignInRequest'];
type EmailVerifyRequest = components['schemas']['EmailVerifyRequest'];

/**
 * Mobile auth client (T-21, ADR-0004). Owns the device session and attaches it to API calls.
 *
 * Server behaviour it is built against (apps/api/src/routes/auth.rs, crates/identity/src/service.rs):
 * - An expired access token gets 401 `SESSION_EXPIRED`. After a rotation the previous access token is
 *   unknown (401 `UNAUTHENTICATED`): a request that raced a refresh is retried with the new token.
 * - `POST /v1/auth/refresh` consumes the refresh token and returns a new pair. Presenting a consumed token
 *   revokes the whole family (401 `SESSION_REVOKED`), except a retry of the immediately previous token within
 *   30 s while its successor is unused. So: exactly one refresh in flight per device (single flight), and a
 *   refresh whose response was lost is retried once at once with the same token, inside the grace window.
 * - 401 `SESSION_REVOKED` anywhere (reuse, logout everywhere, suspension, ban) ends the session on the device.
 *
 * The server enforces all of this; the client only avoids tripping reuse detection by accident.
 * Tokens never reach logs, errors, AsyncStorage or the query cache.
 *
 * On the web several tabs share one stored session (ADR-0009). With a {@link SessionSync} the single flight spans
 * every tab: a refresh runs under a cross-tab lock and first re-reads the store, adopting a pair another tab already
 * rotated to instead of replaying a consumed token (sprint 10 B5); a tab also adopts rotations, sign-ins and
 * sign-outs of other tabs as they happen.
 */

export type AuthStatus = 'loading' | 'signedOut' | 'signedIn';

/** Why the device has no session: the user logged out, or the server ended or no longer knows the session. */
export type SignOutReason = 'user' | 'revoked' | 'invalid';

export interface AuthState {
  readonly status: AuthStatus;
  readonly signOutReason?: SignOutReason;
}

export type IdTokenProvider = 'apple' | 'google';

export type AuthErrorKind =
  /** No session on this device: show the sign-in flow. */
  | 'signed_out'
  /** The server ended the session (revoked, reused, expired family); the device is now signed out. */
  | 'session_ended'
  /** Network failure, rate limit or server error; the session is kept, retry later. */
  | 'unavailable'
  /** The server refused a sign-in (4xx other than 401). */
  | 'rejected';

export class AuthError extends Error {
  override name = 'AuthError';
  constructor(
    readonly kind: AuthErrorKind,
    readonly status?: number,
    readonly code?: string,
  ) {
    // Only the kind and status: never a token or a response body.
    super(status === undefined ? `auth: ${kind}` : `auth: ${kind} (${status})`);
  }
}

export interface AuthClientOptions {
  /** API base URL without a trailing slash; a function so a missing config fails the first request only. */
  baseUrl: () => string;
  store: TokenStore;
  fetch?: typeof fetch;
  /**
   * A fetch whose response body is a readable stream, for {@link AuthClient.fetchStream} (the app passes
   * `expo/fetch`: React Native's built-in fetch cannot stream). Defaults to `fetch`.
   */
  streamFetch?: typeof fetch;
  now?: () => number;
  /** Sent as `X-App-Version` for the server's minimum-version gate (426 `UPGRADE_REQUIRED`). */
  appVersion?: string;
  /** Refresh this long before the access token expires, to avoid a guaranteed 401 round trip. */
  refreshSkewMs?: number;
  /** Upper bound for the best-effort `DELETE /v1/sessions/current` during logout. */
  logoutTimeoutMs?: number;
  /** Coordinates with other app instances sharing `store` (browser tabs, ADR-0009); none on native. */
  sync?: SessionSync;
}

export interface AuthClient {
  /** Loads the session from the secure store. Called automatically by the first request. */
  init(): Promise<void>;
  getState(): AuthState;
  subscribe(listener: (state: AuthState) => void): () => void;
  /**
   * Authorized request to `path` (e.g. `/v1/me`). Refreshes when needed and retries once after a stale or
   * expired access token, so request bodies must be replayable (string, FormData, Blob; not a stream).
   * Non-auth failures come back as the `Response`; auth failures throw {@link AuthError}.
   */
  fetch(path: string, init?: RequestInit): Promise<Response>;
  /**
   * Like {@link fetch}, through the streaming fetch (`streamFetch`), for long-lived responses read as a stream
   * (`GET /v1/events`, ADR-0010). Pass an `AbortSignal` to close it.
   */
  fetchStream(path: string, init?: RequestInit): Promise<Response>;
  /**
   * Like {@link fetch}, for a request whose body carries a credential the server may refuse with 401
   * `UNAUTHENTICATED` (a re-auth proof, T-26). That 401 comes back as the `Response` and never signs the device
   * out; an expired access token is still refreshed and retried once, and `SESSION_REVOKED` still ends the session.
   */
  fetchWithProof(path: string, init?: RequestInit): Promise<Response>;
  /**
   * Exchanges a provider ID token (with the nonce the app generated) for a session. The seam that the
   * Apple (T-19) and Google (T-20) sign-in buttons call; the request DTO is provisional until those routes
   * exist in the OpenAPI document.
   */
  signInWithIdToken(
    provider: IdTokenProvider,
    idToken: string,
    nonce: string,
    options?: SignInOptions,
  ): Promise<void>;
  /**
   * Redeems an email magic link (T-23, ADR-0012): the token from the link plus the PKCE verifier this device kept
   * when it asked for the link. The session is stored like a provider sign-in. A refusal throws
   * {@link AuthError} `rejected` with the server's code (`MAGIC_LINK_INVALID`, `MAGIC_LINK_EXPIRED`,
   * `MAGIC_LINK_USED`, `INVITE_REQUIRED`, `INVITE_INVALID`, `ACCOUNT_SUSPENDED`); 429 and 5xx are `unavailable`.
   */
  signInWithMagicLink(token: string, verifier: string, options?: SignInOptions): Promise<void>;
  /**
   * Development builds only (#306): signs in as a seed user (`POST /v1/dev/sign-in`) against a local server started
   * with `API_DEV_SIGN_IN=true`. Every other server answers 404 (`rejected`). The session is stored like a provider
   * sign-in. Callers must gate on `__DEV__`; the server is the real gate.
   */
  signInAsSeedUser(handle: string): Promise<void>;
  /** Logs out: revokes the session on the server (best effort) and always clears the device. */
  signOut(): Promise<void>;
  /**
   * Logs out everywhere (`POST /v1/sessions/revoke-all`, AC-1.4). Unlike {@link signOut} this is not best
   * effort: the device is cleared only once the server confirms (204) or has already ended the session, so
   * a failure never pretends the other devices were signed out. Failures throw {@link AuthError}.
   */
  signOutEverywhere(): Promise<void>;
}

export interface SignInOptions {
  /** Apple's one-time authorization code from the same sheet (optional; Apple only). */
  authorizationCode?: string;
  /**
   * Sign-up invite code (ADR-0012). The server needs it only when this sign-in creates an account while the
   * sign-up gate is on, and ignores it otherwise.
   */
  inviteCode?: string;
}

/**
 * Provisional body for `POST /v1/auth/{apple|google}` (the subset of the T-19 draft DTO the client sends
 * today); replace with the generated type once the routes are in openapi.json.
 */
interface IdTokenSignInRequest {
  id_token: string;
  nonce: string;
  authorization_code?: string;
  invite_code?: string;
}

const DEFAULT_REFRESH_SKEW_MS = 30_000;
const DEFAULT_LOGOUT_TIMEOUT_MS = 5_000;

export function createAuthClient(options: AuthClientOptions): AuthClient {
  const {
    baseUrl,
    store,
    now = Date.now,
    appVersion,
    refreshSkewMs = DEFAULT_REFRESH_SKEW_MS,
    logoutTimeoutMs = DEFAULT_LOGOUT_TIMEOUT_MS,
    sync,
  } = options;
  const fetchFn = options.fetch ?? ((input, init) => fetch(input, init));
  const streamFetchFn = options.streamFetch ?? fetchFn;

  let session: Session | null = null;
  let state: AuthState = { status: 'loading' };
  /** Bumped on every sign-in and sign-out, so a late refresh cannot resurrect an ended session. */
  let epoch = 0;
  let refreshing: Promise<Session> | null = null;
  let ready: Promise<void> | null = null;
  const listeners = new Set<(state: AuthState) => void>();

  const emit = (next: AuthState) => {
    state = next;
    for (const listener of listeners) listener(state);
  };

  const headers = (init: HeadersInit | undefined, accessToken?: string): Headers => {
    const h = new Headers(init);
    h.set('Accept', 'application/json');
    if (appVersion) h.set('X-App-Version', appVersion);
    // Set last: a caller cannot replace the session credential.
    if (accessToken) h.set('Authorization', `Bearer ${accessToken}`);
    else h.delete('Authorization');
    return h;
  };

  const postJson = (path: string, body: unknown) =>
    fetchFn(`${baseUrl()}${path}`, {
      method: 'POST',
      headers: headers({ 'Content-Type': 'application/json' }),
      body: JSON.stringify(body),
    });

  /** Clears the device. Safe to call more than once; listeners hear about it once. */
  async function endSession(reason: SignOutReason): Promise<void> {
    const wasSignedOut = session === null && state.status === 'signedOut';
    epoch += 1;
    session = null;
    refreshing = null;
    try {
      await store.clear();
    } finally {
      if (!wasSignedOut) emit({ status: 'signedOut', signOutReason: reason });
    }
  }

  async function adopt(next: Session): Promise<void> {
    session = next;
    // Keep the in-memory session even if the keychain write fails: it is valid. On the next launch the
    // stored (previous) refresh token is still accepted within the server's grace window, or the user
    // signs in again; either way nothing leaks.
    await store.save(next).catch(() => undefined);
  }

  async function rotate(observed: Session, startedEpoch: number): Promise<Session> {
    const body: RefreshRequest = { refresh_token: observed.refreshToken };
    let response: Response;
    try {
      response = await postJson('/v1/auth/refresh', body);
    } catch {
      // The request may have reached the server and rotated the family with the response lost. An immediate
      // retry of the same token falls inside the 30 s grace window and is accepted (successor superseded).
      try {
        response = await postJson('/v1/auth/refresh', body);
      } catch {
        throw new AuthError('unavailable');
      }
    }
    if (epoch !== startedEpoch) throw new AuthError('signed_out');
    if (response.status === 401) {
      const code = await problemCode(response);
      if (epoch === startedEpoch)
        await endSession(code === 'SESSION_REVOKED' ? 'revoked' : 'invalid');
      throw new AuthError('session_ended', 401, code);
    }
    if (!response.ok)
      throw new AuthError('unavailable', response.status, await problemCode(response));
    const next = sessionFromTokenPair(await response.json().catch(() => null));
    if (!next) throw new AuthError('unavailable', response.status);
    if (epoch !== startedEpoch) throw new AuthError('signed_out');
    await adopt(next);
    return next;
  }

  /**
   * One rotation across every instance sharing the store. Under the lock the store is read again: a pair another
   * instance rotated to meanwhile is adopted (and only rotated itself when it is due too); a cleared store means
   * another instance signed out. Without a sync this is a plain {@link rotate}.
   */
  function rotateShared(observed: Session, startedEpoch: number): Promise<Session> {
    if (!sync) return rotate(observed, startedEpoch);
    return sync.withRefreshLock(async () => {
      const stored = await store.load().catch(() => undefined);
      if (epoch !== startedEpoch) throw new AuthError('signed_out');
      if (stored === null) {
        await endSession('user');
        throw new AuthError('signed_out');
      }
      if (stored === undefined || stored.refreshToken === observed.refreshToken) {
        return rotate(observed, startedEpoch);
      }
      session = stored;
      if (stored.accessExpiresAt - now() > refreshSkewMs) return stored;
      return rotate(stored, startedEpoch);
    });
  }

  /**
   * Another instance changed the stored session (sync only): adopt its rotation or sign-in, follow its sign-out.
   * Before {@link init} there is nothing to reconcile; init reads the store itself.
   */
  async function adoptExternalChange(): Promise<void> {
    if (!ready) return;
    await ready;
    const startedEpoch = epoch;
    const stored = await store.load().catch(() => undefined);
    if (stored === undefined || epoch !== startedEpoch) return;
    if (stored === null) {
      if (session) await endSession('user');
      return;
    }
    if (session?.refreshToken === stored.refreshToken) return;
    const wasSignedOut = session === null;
    if (wasSignedOut) {
      epoch += 1;
      refreshing = null;
    }
    session = stored;
    if (wasSignedOut) emit({ status: 'signedIn' });
  }
  sync?.subscribe(() => void adoptExternalChange());

  /** Single flight: concurrent callers share one refresh; a caller holding an old session gets the new one. */
  function refreshFrom(observed: Session): Promise<Session> {
    if (session !== observed) {
      return session ? Promise.resolve(session) : Promise.reject(new AuthError('signed_out'));
    }
    if (!refreshing) {
      const flight = rotateShared(observed, epoch);
      refreshing = flight;
      const settle = () => {
        if (refreshing === flight) refreshing = null;
      };
      flight.then(settle, settle);
    }
    return refreshing;
  }

  function init(): Promise<void> {
    if (!ready) {
      const startedEpoch = epoch;
      ready = store
        .load()
        .catch(() => null)
        .then((stored) => {
          if (epoch !== startedEpoch) return; // a sign-in or sign-out happened meanwhile
          session = stored;
          emit(stored ? { status: 'signedIn' } : { status: 'signedOut' });
        });
    }
    return ready;
  }

  const send = (path: string, request: RequestInit, accessToken: string, stream = false) =>
    (stream ? streamFetchFn : fetchFn)(`${baseUrl()}${path}`, {
      ...request,
      headers: headers(request.headers, accessToken),
    }).catch(() => {
      throw new AuthError('unavailable');
    });

  async function authorizedFetch(
    path: string,
    request: RequestInit = {},
    proof = false,
    stream = false,
  ): Promise<Response> {
    await init();
    let current = session;
    if (!current) throw new AuthError('signed_out');

    if (current.accessExpiresAt - now() <= refreshSkewMs) {
      try {
        current = await refreshFrom(current);
      } catch (err) {
        // A transient refresh failure is fine while the access token is still valid.
        if (!(
          err instanceof AuthError &&
          err.kind === 'unavailable' &&
          current.accessExpiresAt > now()
        )) {
          throw err;
        }
      }
    }

    const first = await send(path, request, current.accessToken, stream);
    if (first.status !== 401) return first;

    const code = await problemCode(first);
    if (code === 'SESSION_REVOKED') {
      if (session === current) await endSession('revoked');
      throw new AuthError('session_ended', 401, code);
    }

    // The session is fine; the re-auth token (`X-Reauth`, T-26) is missing or stale. The caller asks again.
    if (code === 'REAUTH_REQUIRED') return first;
    // The server refused the proof in the body, not the session: hand the answer to the caller.
    if (proof && code !== 'SESSION_EXPIRED') return first;

    // SESSION_EXPIRED, or UNAUTHENTICATED because another request already rotated the pair.
    const latest = await refreshFrom(current);
    const second = await send(path, request, latest.accessToken, stream);
    if (second.status !== 401) return second;
    const retryCode = await problemCode(second);
    if (retryCode === 'REAUTH_REQUIRED') return second;
    if (proof && retryCode !== 'SESSION_REVOKED') return second;
    if (session !== latest) return second; // yet another rotation raced us; let the caller retry
    await endSession(retryCode === 'SESSION_REVOKED' ? 'revoked' : 'invalid');
    throw new AuthError('session_ended', 401, retryCode);
  }

  async function signInWithIdToken(
    provider: IdTokenProvider,
    idToken: string,
    nonce: string,
    signInOptions: SignInOptions = {},
  ): Promise<void> {
    const body: IdTokenSignInRequest = { id_token: idToken, nonce };
    if (signInOptions.authorizationCode) body.authorization_code = signInOptions.authorizationCode;
    if (signInOptions.inviteCode) body.invite_code = signInOptions.inviteCode;
    await signInVia(`/v1/auth/${provider}`, body);
  }

  async function signInWithMagicLink(
    token: string,
    verifier: string,
    signInOptions: SignInOptions = {},
  ): Promise<void> {
    const body: EmailVerifyRequest = { token, code_verifier: verifier };
    if (signInOptions.inviteCode) body.invite_code = signInOptions.inviteCode;
    await signInVia('/v1/auth/email/verify', body);
  }

  async function signInAsSeedUser(handle: string): Promise<void> {
    const body: DevSignInRequest = { handle };
    await signInVia('/v1/dev/sign-in', body);
  }

  /** Posts a sign-in body and adopts the token pair it answers. */
  async function signInVia(path: string, body: unknown): Promise<void> {
    await init();
    const startedEpoch = epoch;
    let response: Response;
    try {
      response = await postJson(path, body);
    } catch {
      throw new AuthError('unavailable');
    }
    if (!response.ok) {
      const kind = response.status >= 500 || response.status === 429 ? 'unavailable' : 'rejected';
      throw new AuthError(kind, response.status, await problemCode(response));
    }
    const next = sessionFromTokenPair(await response.json().catch(() => null));
    if (!next) throw new AuthError('unavailable', response.status);
    if (epoch !== startedEpoch) throw new AuthError('signed_out');
    epoch += 1;
    refreshing = null;
    await adopt(next);
    emit({ status: 'signedIn' });
  }

  async function signOut(): Promise<void> {
    await init();
    if (session) {
      const revoke = authorizedFetch('/v1/sessions/current', { method: 'DELETE' }).catch(
        () => undefined,
      );
      let timer: ReturnType<typeof setTimeout> | undefined;
      const timeout = new Promise<void>((resolve) => {
        timer = setTimeout(resolve, logoutTimeoutMs);
      });
      // Best effort: offline or slow, the device is cleared anyway; the server session then idles out.
      await Promise.race([revoke, timeout]);
      clearTimeout(timer);
    }
    await endSession('user');
  }

  async function signOutEverywhere(): Promise<void> {
    const response = await authorizedFetch('/v1/sessions/revoke-all', { method: 'POST' });
    if (!response.ok) {
      throw new AuthError('unavailable', response.status, await problemCode(response));
    }
    await endSession('user');
  }

  return {
    init,
    getState: () => state,
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    fetch: (path, request) => authorizedFetch(path, request),
    fetchStream: (path, request) => authorizedFetch(path, request, false, true),
    fetchWithProof: (path, request) => authorizedFetch(path, request, true),
    signInWithIdToken,
    signInWithMagicLink,
    signInAsSeedUser,
    signOut,
    signOutEverywhere,
  };
}
