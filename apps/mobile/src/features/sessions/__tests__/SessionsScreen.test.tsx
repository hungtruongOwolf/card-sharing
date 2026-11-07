import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { screen, userEvent, waitFor, within } from '@testing-library/react-native';

import type { ColorScheme } from '@card-sharing/tokens';

import { t } from '../../../i18n';
import { FakeAuthServer, problem } from '../../../test/fakeAuthServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { renderThemed, SCHEMES } from '../../../test/render';
import { AuthProvider } from '../../auth/AuthProvider';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { SessionsScreen } from '../SessionsScreen';
import { createSessionsApi } from '../sessionsApi';

const HOUR = 3_600_000;

interface Options {
  scheme?: ColorScheme;
  /** Answer for the next non-GET request instead of the fake server. */
  failNextWrite?: Response;
  /** Fail every request until set to false. */
  offline?: { value: boolean };
}

async function setup({ scheme = 'light', failNextWrite, offline }: Options = {}) {
  const server = new FakeAuthServer();
  const store = memoryStore(sessionFromTokenPair(server.issue({ clientLabel: 'ios 1.0.0' })));
  server.issue({
    clientLabel: 'android 1.0.0',
    ipCountry: 'VN',
    lastSeenAt: server.nowMs - 2 * HOUR,
  });
  let override = failNextWrite;
  const fetchFn: typeof fetch = async (input, init) => {
    if (offline?.value) return problem(503, 'UNAVAILABLE');
    if (override && (init?.method ?? 'GET') !== 'GET') {
      const response = override;
      override = undefined;
      return response;
    }
    return server.fetch(input, init);
  };
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store,
    fetch: fetchFn,
    now: server.now,
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: 0 } },
  });
  await renderThemed(
    <QueryClientProvider client={queryClient}>
      <AuthProvider client={client}>
        <SessionsScreen
          api={createSessionsApi(client.fetch)}
          signOutEverywhere={client.signOutEverywhere}
          signOut={client.signOut}
          onBack={() => undefined}
          now={server.now}
        />
      </AuthProvider>
    </QueryClientProvider>,
    { scheme },
  );
  return { server, client, store, user: userEvent.setup() };
}

/** The confirm button inside the sheet (the screen's own button has the same label). */
const confirmButton = () => {
  const buttons = screen.getAllByRole('button', { name: t('sessions.everywhere.confirm') });
  const last = buttons.at(-1);
  if (!last) throw new Error('no confirm button');
  return last;
};

describe('SessionsScreen', () => {
  it.each(SCHEMES)(
    'lists devices, marks this one and offers no revoke on it (%s)',
    async (scheme) => {
      const { server } = await setup({ scheme });
      const current = await screen.findByTestId(`session-${server.sessionId(1)}`);
      expect(within(current).getByText(t('sessions.device.current'))).toBeTruthy();
      // This device signs out instead of being revoked from the list.
      expect(within(current).queryByRole('button', { name: /^Sign out$/ })).toBeNull();
      expect(within(current).getByRole('button', { name: t('sessions.signOutHere') })).toBeTruthy();
      const other = screen.getByTestId(`session-${server.sessionId(2)}`);
      expect(within(other).getByText('android 1.0.0')).toBeTruthy();
      expect(within(other).getByText(/^Signed in from (Vietnam|VN)$/)).toBeTruthy();
      expect(within(other).getByText('Active 2 hours ago')).toBeTruthy();
    },
  );

  it('revokes another device and refreshes the list', async () => {
    const { server, user } = await setup();
    await screen.findByTestId(`session-${server.sessionId(2)}`);
    await user.press(
      screen.getByRole('button', {
        name: t('sessions.revoke.a11y', { device: 'android 1.0.0' }),
      }),
    );
    await waitFor(() => expect(screen.queryByTestId(`session-${server.sessionId(2)}`)).toBeNull());
    expect(server.isRevoked(2)).toBe(true);
    expect(server.isRevoked(1)).toBe(false);
    expect(screen.getByText(t('sessions.revoked'))).toBeTruthy();
  });

  it('signs out of this device only: the server revokes this session and the tokens are cleared (B8)', async () => {
    const { server, client, store, user } = await setup();
    const current = await screen.findByTestId(`session-${server.sessionId(1)}`);
    await user.press(within(current).getByRole('button', { name: t('sessions.signOutHere') }));
    await waitFor(() =>
      expect(client.getState()).toEqual({ status: 'signedOut', signOutReason: 'user' }),
    );
    expect(server.count('DELETE', '/v1/sessions/current')).toBe(1);
    expect(server.isRevoked(1)).toBe(true);
    expect(server.isRevoked(2)).toBe(false);
    expect(store.current).toBeNull();
  });

  it('asks before logging out everywhere; cancel sends nothing', async () => {
    const { server, user } = await setup();
    await screen.findByTestId(`session-${server.sessionId(1)}`);
    await user.press(screen.getByRole('button', { name: t('sessions.everywhere') }));
    expect(server.count('POST', '/v1/sessions/revoke-all')).toBe(0);
    await user.press(screen.getByRole('button', { name: t('sessions.everywhere.cancel') }));
    expect(server.count('POST', '/v1/sessions/revoke-all')).toBe(0);
  });

  it('logs out everywhere after the confirm and clears the device', async () => {
    const { server, client, store, user } = await setup();
    await screen.findByTestId(`session-${server.sessionId(1)}`);
    await user.press(screen.getByRole('button', { name: t('sessions.everywhere') }));
    await user.press(confirmButton());
    await waitFor(() => expect(client.getState().status).toBe('signedOut'));
    expect(server.count('POST', '/v1/sessions/revoke-all')).toBe(1);
    expect(server.isRevoked(1)).toBe(true);
    expect(server.isRevoked(2)).toBe(true);
    expect(store.current).toBeNull();
  });

  it('stays signed in and says so when log out everywhere fails', async () => {
    const { server, client, store, user } = await setup({
      failNextWrite: problem(503, 'UNAVAILABLE'),
    });
    await screen.findByTestId(`session-${server.sessionId(1)}`);
    await user.press(screen.getByRole('button', { name: t('sessions.everywhere') }));
    await user.press(confirmButton());
    expect(await screen.findByText(t('sessions.error.generic'))).toBeTruthy();
    expect(client.getState().status).toBe('signedIn');
    expect(store.current).not.toBeNull();
    expect(server.isRevoked(2)).toBe(false);
  });

  it('offers a retry when the list cannot load', async () => {
    const offline = { value: true };
    const { server, user } = await setup({ offline });
    await screen.findByText(t('sessions.error.load'));
    offline.value = false;
    await user.press(screen.getByRole('button', { name: t('sessions.retry') }));
    await screen.findByTestId(`session-${server.sessionId(1)}`);
  });
});
