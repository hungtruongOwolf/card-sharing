import { FakeAuthServer } from '../../../test/fakeAuthServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { lastSeenLabel } from '../lastSeen';
import { createSessionsApi, parseSessionList, SessionsError } from '../sessionsApi';

const HOUR = 3_600_000;

function setup() {
  const server = new FakeAuthServer();
  const store = memoryStore(sessionFromTokenPair(server.issue({ clientLabel: 'ios 1.0.0' })));
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store,
    fetch: server.fetch,
    now: server.now,
  });
  return { server, client, store, api: createSessionsApi(client.fetch) };
}

describe('sessions api', () => {
  it('lists device sessions with the current one marked', async () => {
    const { server, api } = setup();
    server.issue({
      clientLabel: 'android 1.0.0',
      ipCountry: 'VN',
      lastSeenAt: server.nowMs - HOUR,
    });
    const sessions = await api.list();
    expect(sessions).toEqual([
      expect.objectContaining({ clientLabel: 'ios 1.0.0', country: null, isCurrent: true }),
      expect.objectContaining({ clientLabel: 'android 1.0.0', country: 'VN', isCurrent: false }),
    ]);
  });

  it('revokes one session by id', async () => {
    const { server, api } = setup();
    server.issue();
    await api.revoke(server.sessionId(2));
    expect(server.isRevoked(2)).toBe(true);
    expect(server.isRevoked(1)).toBe(false);
    expect(server.requests.at(-1)).toMatchObject({
      method: 'DELETE',
      path: `/v1/sessions/${server.sessionId(2)}`,
    });
  });

  it('maps an already revoked session to not_found', async () => {
    const { server, api } = setup();
    server.issue();
    await api.revoke(server.sessionId(2));
    await expect(api.revoke(server.sessionId(2))).rejects.toMatchObject({ kind: 'not_found' });
  });

  it('never sends a non-uuid id into the path', async () => {
    const { server, api } = setup();
    const before = server.requests.length;
    await expect(api.revoke('../me')).rejects.toBeInstanceOf(SessionsError);
    expect(server.requests.length).toBe(before);
  });

  it('drops malformed rows and rejects a body that is not a session list', () => {
    const good = {
      id: '00000000-0000-4000-8000-000000000001',
      client_label: '  ios 1.0.0  ',
      ip_country: 'vn<script>',
      created_at: '2026-11-05T10:00:00Z',
      last_seen_at: '2026-11-05T10:00:00Z',
      is_current: true,
    };
    expect(parseSessionList({ sessions: [good, { ...good, id: 'x' }, null] })).toEqual([
      {
        id: good.id,
        clientLabel: 'ios 1.0.0',
        country: null,
        lastSeenAt: Date.parse(good.last_seen_at),
        isCurrent: true,
      },
    ]);
    expect(parseSessionList({ sessions: 'nope' })).toBeNull();
    expect(parseSessionList(null)).toBeNull();
  });
});

describe('log out everywhere', () => {
  it('revokes every session and clears the device only after the server confirms', async () => {
    const { server, client, store } = setup();
    server.issue();
    await client.signOutEverywhere();
    expect(server.count('POST', '/v1/sessions/revoke-all')).toBe(1);
    expect(server.isRevoked(1)).toBe(true);
    expect(server.isRevoked(2)).toBe(true);
    expect(store.current).toBeNull();
    expect(client.getState()).toEqual({ status: 'signedOut', signOutReason: 'user' });
  });

  it('keeps the session when the server cannot be reached', async () => {
    const { store } = setup();
    const offline = createAuthClient({
      baseUrl: () => 'https://api.test',
      store,
      fetch: () => Promise.reject(new TypeError('Network request failed')),
    });
    await expect(offline.signOutEverywhere()).rejects.toMatchObject({ kind: 'unavailable' });
    expect(store.current).not.toBeNull();
    expect(offline.getState().status).toBe('signedIn');
  });
});

describe('last seen label', () => {
  const now = Date.parse('2026-11-05T10:00:00Z');
  it.each([
    [now + 5_000, 'Active now'],
    [now - 60_000, 'Active now'],
    [now - 5 * 60_000, 'Active 5 minutes ago'],
    [now - HOUR, 'Active 1 hour ago'],
    [now - 3 * 24 * HOUR, 'Active 3 days ago'],
  ])('formats %p', (at, label) => {
    expect(lastSeenLabel(at, now)).toBe(label);
  });
});
