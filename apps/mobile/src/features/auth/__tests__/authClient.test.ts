import * as SecureStore from 'expo-secure-store';

import { FakeAuthServer, problem } from '../../../test/fakeAuthServer';
import { secureStoreMock } from '../../../test/secureStoreMock';
import { AuthError, createAuthClient, type AuthClientOptions, type AuthState } from '../authClient';
import { sessionFromTokenPair, serializeSession } from '../session';
import { createSecureTokenStore, SESSION_KEY } from '../tokenStore';

const NONCE = 'n0nce-0123456789abcdef';

// eslint-disable-next-line @typescript-eslint/no-require-imports -- jest.mock factories must use require()
jest.mock('expo-secure-store', () => require('../../../test/secureStoreMock').secureStoreMock);

const TEN_MINUTES = 10 * 60_000;

function setup(overrides: Partial<AuthClientOptions> = {}) {
  const server = new FakeAuthServer();
  const states: AuthState[] = [];
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: createSecureTokenStore(),
    fetch: server.fetch,
    now: server.now,
    appVersion: '1.2.3',
    ...overrides,
  });
  client.subscribe((s) => states.push(s));
  return { server, client, states };
}

/** Puts a server-issued session in the secure store, as after an earlier sign-in. */
function storeSession(pair: ReturnType<FakeAuthServer['issue']>) {
  const session = sessionFromTokenPair(pair);
  if (!session) throw new Error('bad pair');
  secureStoreMock.__items.set(SESSION_KEY, serializeSession(session));
  return session;
}

const stored = () => {
  const raw = secureStoreMock.__items.get(SESSION_KEY);
  return raw ? (JSON.parse(raw) as { accessToken: string; refreshToken: string }) : null;
};

beforeEach(() => secureStoreMock.__reset());

describe('session store', () => {
  it('loads a stored session into the signed-in state', async () => {
    const { server, client } = setup();
    storeSession(server.issue());
    await client.init();
    expect(client.getState()).toEqual({ status: 'signedIn' });
  });

  it('starts signed out without a stored session and never touches the network', async () => {
    const { server, client } = setup();
    await client.init();
    expect(client.getState()).toEqual({ status: 'signedOut' });
    await expect(client.fetch('/v1/me')).rejects.toMatchObject({ kind: 'signed_out' });
    expect(server.requests).toHaveLength(0);
  });

  it('treats a corrupt stored value as no session', async () => {
    const { client } = setup();
    secureStoreMock.__items.set(SESSION_KEY, '{"accessToken":1}');
    await client.init();
    expect(client.getState().status).toBe('signedOut');
  });

  it('uses the Keychain/Keystore with a this-device-only accessibility class', async () => {
    const { client } = setup();
    await client.signInWithIdToken('apple', 'valid-id-token', NONCE);
    const set = secureStoreMock.__calls.find((c) => c.op === 'set');
    expect(set).toMatchObject({
      key: SESSION_KEY,
      options: { keychainAccessible: SecureStore.AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY },
    });
  });
});

describe('authorized requests', () => {
  it('sends the bearer token and the app version', async () => {
    const { server, client } = setup();
    const session = storeSession(server.issue());
    const res = await client.fetch('/v1/me');
    expect(res.status).toBe(200);
    expect(server.requests[0]).toMatchObject({
      authorization: `Bearer ${session.accessToken}`,
      appVersion: '1.2.3',
    });
  });

  it('does not let a caller replace the Authorization header', async () => {
    const { server, client } = setup();
    const session = storeSession(server.issue());
    await client.fetch('/v1/me', { headers: { Authorization: 'Bearer forged' } });
    expect(server.requests[0]?.authorization).toBe(`Bearer ${session.accessToken}`);
  });

  it('refreshes ahead of expiry and persists the rotated pair', async () => {
    const { server, client } = setup();
    const first = storeSession(server.issue());
    server.advance(TEN_MINUTES - 10_000); // inside the 30 s skew
    const res = await client.fetch('/v1/me');
    expect(res.status).toBe(200);
    expect(server.count('POST', '/v1/auth/refresh')).toBe(1);
    expect(stored()?.refreshToken).not.toBe(first.refreshToken);
    expect(server.requests.at(-1)?.authorization).toBe(`Bearer ${stored()?.accessToken}`);
  });
});

describe('single-flight refresh', () => {
  it('runs exactly one refresh for concurrent requests during expiry', async () => {
    const { server, client } = setup();
    storeSession(server.issue());
    server.advance(TEN_MINUTES + 1);
    const release = server.holdRefreshes();
    const pending = Array.from({ length: 5 }, () => client.fetch('/v1/me'));
    await new Promise((r) => setTimeout(r, 0));
    release();
    const results = await Promise.all(pending);
    expect(results.map((r) => r.status)).toEqual([200, 200, 200, 200, 200]);
    expect(server.count('POST', '/v1/auth/refresh')).toBe(1);
    expect(server.isRevoked()).toBe(false);
  });

  it('recovers from concurrent 401 SESSION_EXPIRED with one refresh (client clock behind)', async () => {
    const server = new FakeAuthServer();
    const clientNow = server.nowMs;
    const skewed = createAuthClient({
      baseUrl: () => server.baseUrl,
      store: createSecureTokenStore(),
      fetch: server.fetch,
      now: () => clientNow, // the device thinks the token is still fresh
      refreshSkewMs: 0,
    });
    storeSession(server.issue());
    server.advance(TEN_MINUTES + 1);
    const results = await Promise.all([
      skewed.fetch('/v1/me'),
      skewed.fetch('/v1/me'),
      skewed.fetch('/v1/me'),
    ]);
    expect(results.map((r) => r.status)).toEqual([200, 200, 200]);
    expect(server.count('POST', '/v1/auth/refresh')).toBe(1);
    expect(server.isRevoked()).toBe(false);
  });

  it('retries a request that raced a rotation with the new token instead of refreshing again', async () => {
    const { server, client } = setup({ refreshSkewMs: 0 });
    storeSession(server.issue());
    await client.init();
    server.advance(TEN_MINUTES + 1);
    // First request rotates the pair; the second was sent with the old token (now UNAUTHENTICATED).
    const [a, b] = await Promise.all([client.fetch('/v1/me'), client.fetch('/v1/me')]);
    expect([a.status, b.status]).toEqual([200, 200]);
    expect(server.count('POST', '/v1/auth/refresh')).toBe(1);
  });

  it('retries a refresh whose response was lost, inside the 30 s grace window', async () => {
    const { server, client } = setup();
    storeSession(server.issue());
    server.advance(TEN_MINUTES + 1);
    server.refreshOverrides.push('lose-response');
    const res = await client.fetch('/v1/me');
    expect(res.status).toBe(200);
    expect(server.count('POST', '/v1/auth/refresh')).toBe(2);
    expect(server.isRevoked()).toBe(false);
    expect(client.getState().status).toBe('signedIn');
  });

  it('keeps the session on a transient refresh failure', async () => {
    const { server, client } = setup();
    const first = storeSession(server.issue());
    server.advance(TEN_MINUTES + 1);
    server.refreshOverrides.push(new Response(null, { status: 503 }));
    await expect(client.fetch('/v1/me')).rejects.toMatchObject({
      kind: 'unavailable',
      status: 503,
    });
    expect(client.getState().status).toBe('signedIn');
    expect(stored()?.refreshToken).toBe(first.refreshToken);
    // The next call refreshes with the same, still unused token.
    expect((await client.fetch('/v1/me')).status).toBe(200);
  });

  it('uses the still-valid access token when an early refresh fails transiently', async () => {
    const { server, client } = setup();
    const first = storeSession(server.issue());
    server.advance(TEN_MINUTES - 10_000);
    server.refreshOverrides.push('network-error', 'network-error');
    const res = await client.fetch('/v1/me');
    expect(res.status).toBe(200);
    expect(server.requests.at(-1)?.authorization).toBe(`Bearer ${first.accessToken}`);
  });
});

describe('revocation and reuse', () => {
  it('signs out when a consumed refresh token is replayed (reuse detection)', async () => {
    const { server, client, states } = setup();
    const pair = server.issue();
    storeSession(pair);
    server.advance(TEN_MINUTES + 1);
    await client.fetch('/v1/me'); // rotates: the stored token is now consumed
    // A stolen copy of the old refresh token is used after the grace window.
    server.advance(60_000);
    const res = await server.fetch(`${server.baseUrl}/v1/auth/refresh`, {
      method: 'POST',
      body: JSON.stringify({ refresh_token: pair.refresh_token }),
    });
    expect(res.status).toBe(401);
    // The device's next request finds the family revoked.
    await expect(client.fetch('/v1/me')).rejects.toMatchObject({
      kind: 'session_ended',
      code: 'SESSION_REVOKED',
    });
    expect(client.getState()).toEqual({ status: 'signedOut', signOutReason: 'revoked' });
    expect(secureStoreMock.__items.has(SESSION_KEY)).toBe(false);
    expect(states.filter((s) => s.status === 'signedOut')).toHaveLength(1);
  });

  it('signs out when the refresh itself reports SESSION_REVOKED', async () => {
    const { server, client } = setup();
    storeSession(server.issue());
    server.revokeAll();
    server.advance(TEN_MINUTES + 1);
    await expect(client.fetch('/v1/me')).rejects.toBeInstanceOf(AuthError);
    expect(client.getState()).toEqual({ status: 'signedOut', signOutReason: 'revoked' });
    expect(secureStoreMock.__items.has(SESSION_KEY)).toBe(false);
  });

  it('signs out when the server forgets the refresh token (UNAUTHENTICATED)', async () => {
    const { server, client } = setup();
    secureStoreMock.__items.set(
      SESSION_KEY,
      serializeSession({ accessToken: 'a', refreshToken: 'unknown', accessExpiresAt: 0 }),
    );
    await expect(client.fetch('/v1/me')).rejects.toMatchObject({ kind: 'session_ended' });
    expect(client.getState()).toEqual({ status: 'signedOut', signOutReason: 'invalid' });
    expect(server.count('POST', '/v1/auth/refresh')).toBe(1);
  });

  it('ends the session on 401 SESSION_REVOKED from any API call', async () => {
    const { server, client } = setup();
    storeSession(server.issue());
    server.revokeAll();
    await expect(client.fetch('/v1/me')).rejects.toMatchObject({ code: 'SESSION_REVOKED' });
    expect(client.getState().signOutReason).toBe('revoked');
    expect(server.count('POST', '/v1/auth/refresh')).toBe(0);
  });

  it('keeps the session on 401 REAUTH_REQUIRED and returns it to the caller (T-26)', async () => {
    const { server, client } = setup();
    storeSession(server.issue());
    server.routes = () => problem(401, 'REAUTH_REQUIRED');
    const response = await client.fetch('/v1/me/deletion', { method: 'POST' });
    expect(response.status).toBe(401);
    expect(client.getState().status).toBe('signedIn');
    expect(server.count('POST', '/v1/auth/refresh')).toBe(0);
  });

  it('returns a refused proof to the caller without refreshing or signing out', async () => {
    const { server, client } = setup();
    storeSession(server.issue());
    server.routes = (r) => (r.path === '/v1/auth/reauth' ? problem(401, 'UNAUTHENTICATED') : null);
    const response = await client.fetchWithProof('/v1/auth/reauth', { method: 'POST' });
    expect(response.status).toBe(401);
    expect(client.getState().status).toBe('signedIn');
    expect(server.count('POST', '/v1/auth/refresh')).toBe(0);
    // The plain fetch treats the same 401 as a stale session.
    await expect(client.fetch('/v1/auth/reauth', { method: 'POST' })).rejects.toBeInstanceOf(
      AuthError,
    );
  });

  it('does not resurrect a session when a refresh finishes after sign-out', async () => {
    const { server, client } = setup({ logoutTimeoutMs: 0 });
    storeSession(server.issue());
    server.advance(TEN_MINUTES + 1);
    const release = server.holdRefreshes();
    const pending = client.fetch('/v1/me');
    await new Promise((r) => setTimeout(r, 0));
    await client.signOut();
    release();
    await expect(pending).rejects.toMatchObject({ kind: 'signed_out' });
    expect(client.getState().status).toBe('signedOut');
    expect(secureStoreMock.__items.has(SESSION_KEY)).toBe(false);
  });
});

describe('logout', () => {
  it('revokes the server session and clears the secure store', async () => {
    const { server, client } = setup();
    const session = storeSession(server.issue());
    await client.signOut();
    expect(server.requests.at(-1)).toMatchObject({
      method: 'DELETE',
      path: '/v1/sessions/current',
      authorization: `Bearer ${session.accessToken}`,
    });
    expect(server.isRevoked()).toBe(true);
    expect(secureStoreMock.__items.has(SESSION_KEY)).toBe(false);
    expect(client.getState()).toEqual({ status: 'signedOut', signOutReason: 'user' });
  });

  it('clears the device even when the server cannot be reached', async () => {
    const server = new FakeAuthServer();
    storeSession(server.issue());
    const client = createAuthClient({
      baseUrl: () => server.baseUrl,
      store: createSecureTokenStore(),
      fetch: () => Promise.reject(new TypeError('Network request failed')),
      now: server.now,
    });
    await client.signOut();
    expect(secureStoreMock.__items.has(SESSION_KEY)).toBe(false);
    expect(client.getState().status).toBe('signedOut');
  });
});

describe('sign-in seam (T-19/T-20)', () => {
  it('stores the session returned for a valid ID token', async () => {
    const { server, client } = setup();
    await client.signInWithIdToken('google', 'valid-id-token', NONCE);
    expect(client.getState()).toEqual({ status: 'signedIn' });
    expect(server.requests[0]).toMatchObject({
      path: '/v1/auth/google',
      authorization: null,
      body: { id_token: 'valid-id-token', nonce: NONCE },
    });
    expect(stored()).not.toBeNull();
    expect((await client.fetch('/v1/me')).status).toBe(200);
  });

  it('forwards the Apple authorization code when given', async () => {
    const { server, client } = setup();
    await client.signInWithIdToken('apple', 'valid-id-token', NONCE, {
      authorizationCode: 'code-1',
    });
    expect(server.requests[0]?.body).toEqual({
      id_token: 'valid-id-token',
      nonce: NONCE,
      authorization_code: 'code-1',
    });
  });

  it('rejects a refused ID token and stays signed out', async () => {
    const { client } = setup();
    await expect(client.signInWithIdToken('apple', 'forged', 'n')).rejects.toMatchObject({
      kind: 'rejected',
      status: 401,
    });
    expect(client.getState().status).toBe('signedOut');
    expect(secureStoreMock.__items.has(SESSION_KEY)).toBe(false);
  });

  it('never puts a token into an error message', async () => {
    const { server, client } = setup();
    storeSession(server.issue());
    server.revokeAll();
    const err = await client.fetch('/v1/me').catch((e: unknown) => e);
    expect(String((err as Error).message)).not.toMatch(/access-|refresh-/);
  });
});
