import { QueryClient } from '@tanstack/react-query';

import { problem } from '../../../test/fakeAuthServer';
import { DEFAULT_PREFS, FakeNotificationsServer } from '../../../test/fakeNotificationsServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import {
  createNotificationsApi,
  isTimeZone,
  NotificationsError,
  parseNotificationPrefs,
} from '../notificationsApi';
import { shiftClockTime } from '../quietHours';
import { deviceTimeZone, NOTIFICATION_PREFS_QUERY_KEY, syncTimeZone } from '../timeZone';

function setup() {
  const server = new FakeNotificationsServer();
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.auth.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  return { server, api: createNotificationsApi(client.fetch) };
}

const PATH = '/v1/me/notification-preferences';

describe('notifications api', () => {
  it('reads the defaults until something is saved', async () => {
    const { api } = setup();
    await expect(api.get()).resolves.toEqual({
      types: { cardNew: true, reaction: true, friend: true, cardReview: true },
      quietHours: { enabled: true, start: '22:00', end: '08:00' },
      timeZone: null,
    });
  });

  it('replaces every field with exactly the contract body and returns the stored value', async () => {
    const { server, api } = setup();
    const saved = await api.put({
      types: { cardNew: false, reaction: true, friend: true, cardReview: false },
      quietHours: { enabled: true, start: '23:30', end: '07:00' },
      timeZone: 'Europe/Paris',
    });
    expect(saved.timeZone).toBe('Europe/Paris');
    expect(server.requests.at(-1)).toMatchObject({
      method: 'PUT',
      path: PATH,
      contentType: 'application/json',
      body: {
        types: { card_new: false, reaction: true, friend: true, card_review: false },
        quiet_hours: { enabled: true, start: '23:30', end: '07:00' },
        time_zone: 'Europe/Paris',
      },
    });
  });

  it('maps a refused save to invalid and a throttled one to rate_limited', async () => {
    const { server, api } = setup();
    const prefs = await api.get();
    await expect(
      api.put({ ...prefs, quietHours: { enabled: true, start: '08:00', end: '08:00' } }),
    ).rejects.toMatchObject({ kind: 'invalid', status: 400 });
    server.overrides.set(`PUT ${PATH}`, problem(429, 'RATE_LIMITED'));
    await expect(api.put(prefs)).rejects.toMatchObject({ kind: 'rate_limited' });
    server.overrides.set(`GET ${PATH}`, problem(503, 'TEMPORARILY_UNAVAILABLE'));
    await expect(api.get()).rejects.toBeInstanceOf(NotificationsError);
  });

  it('refuses a malformed body instead of guessing', () => {
    expect(parseNotificationPrefs(DEFAULT_PREFS)).not.toBeNull();
    expect(parseNotificationPrefs({ ...DEFAULT_PREFS, types: { card_new: true } })).toBeNull();
    expect(
      parseNotificationPrefs({
        ...DEFAULT_PREFS,
        quiet_hours: { enabled: true, start: '9:00', end: '08:00' },
      }),
    ).toBeNull();
    expect(
      parseNotificationPrefs({ ...DEFAULT_PREFS, time_zone: '<script>' })?.timeZone,
    ).toBeNull();
    expect(parseNotificationPrefs('nope')).toBeNull();
  });

  it('accepts IANA names only', () => {
    expect(isTimeZone('Europe/Paris')).toBe(true);
    expect(isTimeZone('America/Argentina/Buenos_Aires')).toBe(true);
    expect(isTimeZone('Etc/GMT+7')).toBe(true);
    expect(isTimeZone('UTC')).toBe(true);
    expect(isTimeZone('../etc')).toBe(false);
    expect(isTimeZone('')).toBe(false);
    expect(isTimeZone(42)).toBe(false);
  });

  it('reads the device zone from Intl', () => {
    const zone = deviceTimeZone();
    expect(zone === null || isTimeZone(zone)).toBe(true);
  });

  it('steps quiet-hour times by 30 minutes around midnight', () => {
    expect(shiftClockTime('22:00', 1)).toBe('22:30');
    expect(shiftClockTime('23:30', 1)).toBe('00:00');
    expect(shiftClockTime('00:00', -1)).toBe('23:30');
    expect(shiftClockTime('07:15', -2)).toBe('06:15');
  });
});

describe('syncTimeZone', () => {
  it('writes the zone back with every other field unchanged', async () => {
    const { server, api } = setup();
    server.prefs = { ...server.prefs, types: { ...server.prefs.types, reaction: false } };
    const queryClient = new QueryClient();
    const stored = await syncTimeZone(api, 'Asia/Ho_Chi_Minh', queryClient);
    expect(stored.timeZone).toBe('Asia/Ho_Chi_Minh');
    expect(server.prefs).toEqual({
      ...DEFAULT_PREFS,
      types: { ...DEFAULT_PREFS.types, reaction: false },
      time_zone: 'Asia/Ho_Chi_Minh',
    });
    expect(queryClient.getQueryData(NOTIFICATION_PREFS_QUERY_KEY)).toEqual(stored);
  });

  it('sends nothing when the server already has the zone', async () => {
    const { server, api } = setup();
    server.prefs = { ...server.prefs, time_zone: 'Europe/Paris' };
    await syncTimeZone(api, 'Europe/Paris', new QueryClient());
    expect(server.count('PUT', PATH)).toBe(0);
  });
});
