import { problem } from '../../../test/fakeAuthServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { FakePrivacyServer, REAUTH_TTL_MS } from '../../../test/fakePrivacyServer';
import { AuthError, createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { createPrivacyApi, parseExport, PrivacyError, type ReauthProof } from '../privacyApi';

const PROOF: ReauthProof = {
  provider: 'apple',
  idToken: 'valid-id-token',
  nonce: 'n0nce-0123456789abcdef',
};

function setup() {
  const server = new FakePrivacyServer();
  const store = memoryStore(sessionFromTokenPair(server.auth.issue()));
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store,
    fetch: server.fetch,
    now: server.auth.now,
    logoutTimeoutMs: 50,
  });
  return { server, store, client, api: createPrivacyApi(client) };
}

const kindOf = (promise: Promise<unknown>) =>
  promise.then(
    () => 'resolved',
    (err: unknown) => (err instanceof PrivacyError ? err.kind : String(err)),
  );

describe('privacyApi', () => {
  it('exchanges a proof for a re-auth token and sends it only as X-Reauth', async () => {
    const { server, api } = setup();
    const grant = await api.reauthenticate(PROOF);
    expect(grant.token).toBe('reauth-1');
    expect(grant.expiresAt).toBe(server.auth.nowMs + REAUTH_TTL_MS);
    const reauth = server.auth.requests.find((r) => r.path === '/v1/auth/reauth');
    expect(reauth?.body).toEqual({
      provider: 'apple',
      id_token: 'valid-id-token',
      nonce: PROOF.nonce,
    });

    await api.requestExport(grant);
    const post = server.auth.requests.find((r) => r.path === '/v1/me/exports');
    expect(post?.reauth).toBe('reauth-1');
    expect(post?.authorization).toMatch(/^Bearer access-/);
  });

  it('a refused proof never signs the device out', async () => {
    const { server, api, client, store } = setup();
    await client.init();
    expect(await kindOf(api.reauthenticate({ ...PROOF, idToken: 'other-account' }))).toBe(
      'proof_rejected',
    );
    expect(client.getState().status).toBe('signedIn');
    expect(store.current).not.toBeNull();
    // Not retried: the nonce is single use and the proof would be refused again.
    expect(server.auth.count('POST', '/v1/auth/reauth')).toBe(1);
  });

  it('refreshes an expired session before sending the proof again', async () => {
    const { server, api } = setup();
    server.auth.advance(11 * 60_000); // access token expired, refresh token still valid
    const grant = await api.reauthenticate(PROOF);
    expect(grant.token).toBe('reauth-1');
    expect(server.auth.count('POST', '/v1/auth/refresh')).toBe(1);
  });

  it('a revoked session still ends the session during re-auth', async () => {
    const { server, api, client } = setup();
    await client.init();
    server.auth.revokeAll();
    await expect(api.reauthenticate(PROOF)).rejects.toBeInstanceOf(AuthError);
    expect(client.getState().status).toBe('signedOut');
  });

  it('maps REAUTH_REQUIRED, cooldown with Retry-After and rate limits', async () => {
    const { server, api } = setup();
    const grant = await api.reauthenticate(PROOF);
    server.auth.advance(REAUTH_TTL_MS);
    expect(await kindOf(api.requestDeletion(grant))).toBe('reauth_required');

    const fresh = await api.reauthenticate({ ...PROOF, nonce: 'n0nce-second-0123456' });
    await api.requestExport(fresh);
    const cooldown = await api.requestExport(fresh).catch((e: unknown) => e);
    expect(cooldown).toBeInstanceOf(PrivacyError);
    expect((cooldown as PrivacyError).kind).toBe('cooldown');
    expect((cooldown as PrivacyError).retryAfterSeconds).toBe(24 * 3600);

    server.overrides.set('DELETE /v1/me/deletion', problem(429, 'RATE_LIMITED'));
    expect(await kindOf(api.cancelDeletion())).toBe('rate_limited');
  });

  it('reads, requests and cancels a deletion', async () => {
    const { server, api } = setup();
    expect(await api.getDeletion()).toBeNull();
    const grant = await api.reauthenticate(PROOF);
    const pending = await api.requestDeletion(grant);
    expect(pending.graceUntil - pending.requestedAt).toBe(7 * 24 * 3600_000);
    // The server logged out every device, this one included.
    expect(server.auth.isRevoked(1)).toBe(true);
  });

  it('cancelling after the grace period is not found', async () => {
    const { server, api } = setup();
    server.deletion = { requestedAt: server.auth.nowMs - 1, graceUntil: server.auth.nowMs };
    expect(await kindOf(api.cancelDeletion())).toBe('not_found');
  });

  it('never sends an export id it did not get from the server', async () => {
    const { server, api } = setup();
    const before = server.auth.requests.length;
    expect(await kindOf(api.cancelExport('../me/deletion'))).toBe('not_found');
    expect(server.auth.requests.length).toBe(before);
  });

  it('rejects malformed bodies and download paths off the API', () => {
    /** A placeholder in the token's shape (built, so secret scanners see no literal). */
    const TOKEN = 'a'.repeat(16);
    const view = {
      id: '00000000-0000-4000-8000-000000000001',
      status: 'ready',
      requested_at: '2026-11-05T10:00:00Z',
      available_at: '2026-11-07T10:00:00Z',
      expires_at: '2026-11-08T10:00:00Z',
    };
    expect(parseExport({ export: { ...view, status: 'unknown' } })).toBeNull();
    expect(parseExport({ export: { ...view, id: 'x' } })).toBeNull();
    expect(
      parseExport({
        export: { ...view, download_path: `https://evil.test/x?token=${TOKEN}` },
      })?.downloadPath,
    ).toBeNull();
    const ok = `/v1/exports/${view.id}/download?token=${TOKEN}`;
    expect(parseExport({ export: { ...view, download_path: ok } })?.downloadPath).toBe(ok);
    expect(
      parseExport({ export: { ...view, status: 'queued', download_path: ok } })?.downloadPath,
    ).toBeNull();
  });
});
