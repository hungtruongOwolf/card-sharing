import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { screen, userEvent, waitFor } from '@testing-library/react-native';

import type { ColorScheme } from '@card-sharing/tokens';

import { t } from '../../../i18n';
import { FakePrivacyServer } from '../../../test/fakePrivacyServer';
import { fakeProvider, memoryStore } from '../../../test/onboardingHarness';
import { renderThemed, SCHEMES } from '../../../test/render';
import { AuthProvider } from '../../auth/AuthProvider';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import type { SignInProvider } from '../../onboarding/signInProviders';
import { DeleteAccountScreen } from '../DeleteAccountScreen';
import { createPrivacyApi, type PendingDeletion } from '../privacyApi';

async function setup({
  scheme = 'light' as ColorScheme,
  providers = [fakeProvider('apple'), fakeProvider('google')] as readonly SignInProvider[],
} = {}) {
  const server = new FakePrivacyServer();
  const store = memoryStore(sessionFromTokenPair(server.auth.issue()));
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store,
    fetch: server.fetch,
    now: server.auth.now,
    logoutTimeoutMs: 50,
  });
  const signedOut = jest.fn();
  const onDeleted = jest.fn(async (_deletion: PendingDeletion) => {
    await client.signOut();
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: 0 } },
  });
  await renderThemed(
    <QueryClientProvider client={queryClient}>
      <AuthProvider client={client} onSignedOut={signedOut}>
        <DeleteAccountScreen
          api={createPrivacyApi(client)}
          providers={providers}
          onDeleted={onDeleted}
          onBack={() => undefined}
        />
      </AuthProvider>
    </QueryClientProvider>,
    { scheme },
  );
  return { server, client, store, onDeleted, signedOut, user: userEvent.setup() };
}

const deleteButton = () => screen.getByRole('button', { name: t('deleteAccount.confirm') });

describe('DeleteAccountScreen', () => {
  it.each(SCHEMES)('explains every consequence before anything happens (%s)', async (scheme) => {
    const { server } = await setup({ scheme });
    expect(screen.getByText(t('deleteAccount.intro'))).toBeTruthy();
    for (const key of [
      'deleteAccount.consequence.logout',
      'deleteAccount.consequence.hidden',
      'deleteAccount.consequence.boards',
      'deleteAccount.consequence.grace',
      'deleteAccount.consequence.final',
    ] as const) {
      expect(screen.getByText(`• ${t(key)}`)).toBeTruthy();
    }
    expect(server.auth.requests).toHaveLength(0);
  });

  it('re-authenticates, deletes with X-Reauth and clears the device (widget included)', async () => {
    const { server, client, store, onDeleted, signedOut, user } = await setup();
    await user.press(deleteButton());
    expect(await screen.findByText(t('deleteAccount.reauth.title'))).toBeTruthy();
    expect(server.auth.count('POST', '/v1/me/deletion')).toBe(0);

    await user.press(screen.getByRole('button', { name: t('reauth.apple') }));
    await waitFor(() => expect(onDeleted).toHaveBeenCalledTimes(1));
    const post = server.auth.requests.find(
      (r) => r.method === 'POST' && r.path === '/v1/me/deletion',
    );
    expect(post?.reauth).toBe('reauth-1');
    expect(onDeleted.mock.calls[0]?.[0].graceUntil).toBe(server.deletion?.graceUntil);
    await waitFor(() => expect(client.getState().status).toBe('signedOut'));
    expect(store.current).toBeNull();
    // Sign-out runs the device cleanup that empties the widget token store (AC-16.2).
    expect(signedOut).toHaveBeenCalled();
  });

  it('closing the confirmation sends nothing', async () => {
    const { server, onDeleted, user } = await setup();
    await user.press(deleteButton());
    await user.press(await screen.findByRole('button', { name: t('common.close') }));
    await waitFor(() => expect(screen.queryByText(t('deleteAccount.reauth.title'))).toBeNull());
    expect(server.auth.count('POST', '/v1/auth/reauth')).toBe(0);
    expect(server.auth.count('POST', '/v1/me/deletion')).toBe(0);
    expect(onDeleted).not.toHaveBeenCalled();
    expect(deleteButton()).toBeTruthy();
  });

  it('a proof of another account is refused and the user stays signed in', async () => {
    const other = fakeProvider('google', {
      idToken: 'other-account',
      nonce: 'n0nce-0123456789abcdef',
    });
    const { server, client, onDeleted, user } = await setup({
      providers: [fakeProvider('apple', 'unavailable'), other],
    });
    await user.press(deleteButton());
    await user.press(await screen.findByRole('button', { name: t('reauth.apple') }));
    expect(await screen.findByText(t('reauth.unavailable'))).toBeTruthy();

    await user.press(screen.getByRole('button', { name: t('reauth.google') }));
    expect(await screen.findByText(t('reauth.rejected'))).toBeTruthy();
    expect(server.auth.count('POST', '/v1/me/deletion')).toBe(0);
    expect(client.getState().status).toBe('signedIn');
    expect(onDeleted).not.toHaveBeenCalled();
  });
});
