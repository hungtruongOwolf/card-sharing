import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, waitFor } from '@testing-library/react-native';
import type { AppStateStatus } from 'react-native';
import { act } from 'react';

import { problem } from '../../../test/fakeAuthServer';
import { FakeNotificationsServer } from '../../../test/fakeNotificationsServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { memoryKeyValueStore } from '../../drafts/draftStore';
import { createNotificationsApi } from '../notificationsApi';
import { createSyncedTimeZoneStore, SYNCED_TIME_ZONE_ENTRY } from '../syncedTimeZone';
import { TimeZoneSync } from '../TimeZoneSync';

const PATH = '/v1/me/notification-preferences';

function fakeAppState() {
  const listeners: ((state: AppStateStatus) => void)[] = [];
  return {
    currentState: 'active' as AppStateStatus,
    addEventListener: (_: 'change', listener: (state: AppStateStatus) => void) => {
      listeners.push(listener);
      return { remove: () => listeners.splice(listeners.indexOf(listener), 1) };
    },
    emit(state: AppStateStatus) {
      for (const listener of [...listeners]) listener(state);
    },
  };
}

async function setup(
  zone: { value: string | null },
  { synced = null, clock = { now: 0 } }: { synced?: string | null; clock?: { now: number } } = {},
) {
  const server = new FakeNotificationsServer();
  const kv = memoryKeyValueStore();
  if (synced) kv.items.set(SYNCED_TIME_ZONE_ENTRY, synced);
  const store = createSyncedTimeZoneStore(kv);
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.auth.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  const appState = fakeAppState();
  const api = createNotificationsApi(client.fetch);
  const timeZone = () => zone.value;
  await render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { gcTime: Infinity } } })}
    >
      <TimeZoneSync
        api={api}
        store={store}
        now={() => clock.now}
        appState={appState as unknown as Parameters<typeof TimeZoneSync>[0]['appState']}
        timeZone={timeZone}
      />
    </QueryClientProvider>,
  );
  const foreground = async () => {
    await act(async () => {
      appState.emit('background');
      appState.emit('active');
      await Promise.resolve();
    });
  };
  return { server, foreground, kv };
}

async function setupWith(
  server: FakeNotificationsServer,
  zone: { value: string | null },
  clock: { now: number },
) {
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.auth.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  const appState = fakeAppState();
  await render(
    <QueryClientProvider client={new QueryClient()}>
      <TimeZoneSync
        api={createNotificationsApi(client.fetch)}
        store={createSyncedTimeZoneStore(memoryKeyValueStore())}
        now={() => clock.now}
        appState={appState as unknown as Parameters<typeof TimeZoneSync>[0]['appState']}
        timeZone={() => zone.value}
      />
    </QueryClientProvider>,
  );
  return {
    foreground: async () => {
      await act(async () => {
        appState.emit('background');
        appState.emit('active');
        await Promise.resolve();
      });
    },
  };
}

describe('TimeZoneSync', () => {
  it('sends the device zone on launch and again only when it changes', async () => {
    const zone = { value: 'Europe/Paris' as string | null };
    const { server, foreground } = await setup(zone);
    await waitFor(() => expect(server.prefs.time_zone).toBe('Europe/Paris'));
    expect(server.count('PUT', PATH)).toBe(1);

    await foreground();
    await waitFor(() => expect(server.count('GET', PATH)).toBe(1));
    expect(server.count('PUT', PATH)).toBe(1);

    zone.value = 'Asia/Ho_Chi_Minh';
    await foreground();
    await waitFor(() => expect(server.prefs.time_zone).toBe('Asia/Ho_Chi_Minh'));
    expect(server.count('PUT', PATH)).toBe(2);
  });

  it('retries on the next foreground after a failure', async () => {
    const zone = { value: 'Europe/Paris' as string | null };
    const { server, foreground } = await setup(zone);
    await waitFor(() => expect(server.prefs.time_zone).toBe('Europe/Paris'));
    zone.value = 'America/New_York';
    server.overrides.set(`GET ${PATH}`, problem(503, 'TEMPORARILY_UNAVAILABLE'));
    await foreground();
    await waitFor(() => expect(server.count('GET', PATH)).toBe(2));
    expect(server.prefs.time_zone).toBe('Europe/Paris');
    await foreground();
    await waitFor(() => expect(server.prefs.time_zone).toBe('America/New_York'));
  });

  it('does not retry a zone the server refuses until the zone changes', async () => {
    const zone = { value: 'Mars/Olympus' as string | null };
    const { server, foreground } = await setup(zone);
    await waitFor(() => expect(server.count('PUT', PATH)).toBe(1));
    await foreground();
    await act(async () => {
      await Promise.resolve();
    });
    expect(server.count('GET', PATH)).toBe(1);
    expect(server.prefs.time_zone).toBeNull();
  });

  it('sends nothing on a launch whose zone this device already sent (a web reload, B7)', async () => {
    const { server, foreground } = await setup(
      { value: 'Europe/Paris' },
      { synced: 'Europe/Paris' },
    );
    await foreground();
    await act(async () => {
      await Promise.resolve();
    });
    expect(server.requests).toHaveLength(0);
  });

  it('remembers the zone it sent and syncs again when the device zone differs', async () => {
    const { server, kv } = await setup({ value: 'Asia/Ho_Chi_Minh' }, { synced: 'Europe/Paris' });
    await waitFor(() => expect(server.prefs.time_zone).toBe('Asia/Ho_Chi_Minh'));
    await waitFor(() => expect(kv.items.get(SYNCED_TIME_ZONE_ENTRY)).toBe('Asia/Ho_Chi_Minh'));
  });

  it('leaves the endpoint alone after a 429 until Retry-After has passed (B7)', async () => {
    const clock = { now: 1_000_000 };
    const zone = { value: 'Europe/Paris' as string | null };
    const server = new FakeNotificationsServer();
    server.overrides.set(
      `GET ${PATH}`,
      new Response(JSON.stringify({ type: 'about:blank', status: 429, code: 'RATE_LIMITED' }), {
        status: 429,
        headers: { 'Content-Type': 'application/problem+json', 'Retry-After': '120' },
      }),
    );
    const { foreground } = await setupWith(server, zone, clock);
    await waitFor(() => expect(server.count('GET', PATH)).toBe(1));
    await foreground();
    clock.now += 119_000;
    await foreground();
    expect(server.count('GET', PATH)).toBe(1);
    clock.now += 2_000;
    await foreground();
    await waitFor(() => expect(server.prefs.time_zone).toBe('Europe/Paris'));
    expect(server.count('GET', PATH)).toBe(2);
  });

  it('sends nothing without a device zone', async () => {
    const { server } = await setup({ value: null });
    await act(async () => {
      await Promise.resolve();
    });
    expect(server.requests).toHaveLength(0);
  });
});
