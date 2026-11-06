import { screen, userEvent, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { FakeOnboardingServer } from '../../../test/fakeOnboardingServer';
import { renderOnboarding } from '../../../test/onboardingHarness';
import { createDevSignIn, DevSignInError, parseSeedUsers } from '../devSignIn';

const SEEDS = ['alice.demo', 'bob.demo', 'mia.demo'];

function devServer(handles: string[] | null = SEEDS) {
  const server = new FakeOnboardingServer();
  server.auth.devSeedHandles = handles ? new Set(handles) : null;
  return server;
}

const devSignInFor =
  (server: FakeOnboardingServer) =>
  (authClient: Parameters<typeof createDevSignIn>[0]['authClient']) =>
    createDevSignIn({ baseUrl: () => server.baseUrl, authClient, fetch: server.fetch });

describe('development sign-in (#306)', () => {
  it('lists the seed users and signs in as one like a provider sign-in', async () => {
    const server = devServer();
    server.resetAccount('active');
    const user = userEvent.setup();
    const { authClient } = await renderOnboarding(server, { devSignIn: devSignInFor(server) });

    await user.press(await screen.findByRole('button', { name: t('devSignIn.open') }));
    const bob = await screen.findByRole('button', {
      name: t('devSignIn.user', { name: 'bob (demo)', handle: 'bob.demo' }),
    });
    expect(
      screen.getByRole('button', {
        name: t('devSignIn.userMinor', { name: 'mia (demo)', handle: 'mia.demo' }),
      }),
    ).toBeTruthy();
    await user.press(bob);
    await waitFor(() => expect(authClient.getState().status).toBe('signedIn'));
    const post = server.auth.requests.find((r) => r.path === '/v1/dev/sign-in');
    expect(post?.body).toEqual({ handle: 'bob.demo' });
    expect(post?.authorization).toBeNull();
  });

  it('says so when the server has no development sign-in (staging, production)', async () => {
    const server = devServer(null);
    const user = userEvent.setup();
    const { authClient } = await renderOnboarding(server, { devSignIn: devSignInFor(server) });
    await user.press(await screen.findByRole('button', { name: t('devSignIn.open') }));
    expect(await screen.findByText(t('devSignIn.disabled'))).toBeTruthy();
    expect(authClient.getState().status).toBe('signedOut');
  });

  it('has no entry without the dev wiring (release builds pass null)', async () => {
    const server = devServer();
    await renderOnboarding(server);
    await screen.findByTestId('onboarding-sign-in');
    expect(screen.queryByTestId('dev-sign-in')).toBeNull();
  });

  it('has no entry when __DEV__ is false, even if wired', async () => {
    const server = devServer();
    const g = globalThis as { __DEV__?: boolean };
    const previous = g.__DEV__;
    g.__DEV__ = false;
    try {
      await renderOnboarding(server, { devSignIn: devSignInFor(server) });
      await screen.findByTestId('onboarding-sign-in');
      expect(screen.queryByTestId('dev-sign-in')).toBeNull();
    } finally {
      g.__DEV__ = previous;
    }
  });

  it('validates the seed list and maps a missing route to disabled', async () => {
    expect(
      parseSeedUsers({
        users: [
          { handle: 'alice.demo', display_name: 'Alice', minor: false },
          { handle: 'Not A Handle', display_name: 'x', minor: false },
          { handle: 'mia.demo', display_name: 'Mia' },
        ],
      }),
    ).toEqual([{ handle: 'alice.demo', displayName: 'Alice', minor: false }]);
    expect(parseSeedUsers({})).toBeNull();

    const server = devServer(null);
    const dev = createDevSignIn({
      baseUrl: () => server.baseUrl,
      authClient: { signInAsSeedUser: jest.fn() },
      fetch: server.fetch,
    });
    await expect(dev.listSeedUsers()).rejects.toEqual(new DevSignInError('disabled'));
  });
});
