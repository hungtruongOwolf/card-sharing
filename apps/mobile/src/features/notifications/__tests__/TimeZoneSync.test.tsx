import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, waitFor } from '@testing-library/react-native';
import type { AppStateStatus } from 'react-native';
import { act } from 'react';

import { problem } from '../../../test/fakeAuthServer';
import { FakeNotificationsServer } from '../../../test/fakeNotificationsServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { createNotificationsApi } from '../notificationsApi';
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

async function setup(zone: { value: string | null }) {
  const server = new FakeNotificationsServer();
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
  return { server, foreground };
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

  it('sends nothing without a device zone', async () => {
    const { server } = await setup({ value: null });
    await act(async () => {
      await Promise.resolve();
    });
    expect(server.requests).toHaveLength(0);
  });
});
