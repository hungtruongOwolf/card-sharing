import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { screen, userEvent, waitFor } from '@testing-library/react-native';

import type { ColorScheme } from '@card-sharing/tokens';

import { t } from '../../../i18n';
import { downloadToken, EXPORT_DELAY_MS, FakePrivacyServer } from '../../../test/fakePrivacyServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { renderThemed, SCHEMES } from '../../../test/render';
import { AuthProvider } from '../../auth/AuthProvider';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import type { SignInProvider } from '../../onboarding/signInProviders';
import { DataExportScreen } from '../DataExportScreen';
import { createExportStore, EXPORT_KEY, type ExportStore } from '../exportStore';
import { memoryKeyValueStore } from '../../drafts/draftStore';
import { createPrivacyApi } from '../privacyApi';
import { formatDateTime } from '../privacyMessages';

let proofs = 0;
/** The Apple seam with a new nonce per sheet (nonces are single use on the server). */
function freshProofs(): SignInProvider {
  return {
    id: 'apple',
    isAvailable: () => true,
    getCredential: async () => {
      proofs += 1;
      return { idToken: 'valid-id-token', nonce: `n0nce-${String(proofs).padStart(16, '0')}` };
    },
  };
}

async function setup({
  scheme = 'light' as ColorScheme,
  server = new FakePrivacyServer(),
  store = createExportStore(memoryKeyValueStore()) as ExportStore,
} = {}) {
  const auth = memoryStore(sessionFromTokenPair(server.auth.issue()));
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: auth,
    fetch: server.fetch,
    now: server.auth.now,
    logoutTimeoutMs: 50,
  });
  const openUrl = jest.fn(async (_url: string) => true);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: 0 } },
  });
  const view = await renderThemed(
    <QueryClientProvider client={queryClient}>
      <AuthProvider client={client}>
        <DataExportScreen
          api={createPrivacyApi(client)}
          providers={[freshProofs()]}
          store={store}
          baseUrl={() => server.baseUrl}
          openUrl={openUrl}
          onBack={() => undefined}
          now={server.auth.now}
        />
      </AuthProvider>
    </QueryClientProvider>,
    { scheme },
  );
  return { server, store, openUrl, view, user: userEvent.setup() };
}

const confirmWithApple = async (user: ReturnType<typeof userEvent.setup>) =>
  user.press(await screen.findByRole('button', { name: t('reauth.apple') }));

describe('DataExportScreen', () => {
  it.each(SCHEMES)('requests after re-auth and shows the 48-hour wait (%s)', async (scheme) => {
    const { server, store, user } = await setup({ scheme });
    await user.press(await screen.findByRole('button', { name: t('dataExport.request') }));
    expect(server.auth.count('POST', '/v1/me/exports')).toBe(0); // not before re-auth
    await confirmWithApple(user);

    const availableAt = server.auth.nowMs + EXPORT_DELAY_MS;
    expect(
      await screen.findByText(t('dataExport.waiting', { date: formatDateTime(availableAt) })),
    ).toBeTruthy();
    expect(server.auth.requests.find((r) => r.path === '/v1/me/exports')?.reauth).toBe('reauth-1');
    expect(screen.queryByRole('button', { name: t('dataExport.download') })).toBeNull();
    expect((await store.load())?.status).toBe('queued');
  });

  it('cancels while waiting, without re-auth', async () => {
    const { server, store, user } = await setup();
    await user.press(await screen.findByRole('button', { name: t('dataExport.request') }));
    await confirmWithApple(user);
    await user.press(await screen.findByRole('button', { name: t('dataExport.cancel') }));
    expect(await screen.findByText(t('dataExport.cancelled'))).toBeTruthy();
    expect(server.exports[0]?.status).toBe('cancelled');
    expect(server.auth.count('POST', '/v1/auth/reauth')).toBe(1);
    expect(await store.load()).toBeNull();
    expect(screen.getByRole('button', { name: t('dataExport.request') })).toBeTruthy();
  });

  it('downloads after the delay with a fresh re-auth and opens the single-use link', async () => {
    const server = new FakePrivacyServer();
    const store = createExportStore(memoryKeyValueStore());
    const requestedAt = server.auth.nowMs - EXPORT_DELAY_MS - 1;
    const id = '00000000-0000-4000-8000-0000000000cc';
    server.exports.push({
      id,
      status: 'queued',
      requestedAt,
      availableAt: requestedAt + EXPORT_DELAY_MS,
      expiresAt: null,
    });
    server.markReady(id); // the worker built it after the 48 hours
    await store.save({
      id,
      status: 'queued',
      requestedAt,
      availableAt: requestedAt + EXPORT_DELAY_MS,
    });

    const { openUrl, user } = await setup({ server, store });
    await user.press(await screen.findByRole('button', { name: t('dataExport.download') }));
    expect(server.auth.count('GET', `/v1/me/exports/${id}`)).toBe(0); // not before re-auth
    await confirmWithApple(user);
    await waitFor(() => expect(openUrl).toHaveBeenCalledTimes(1));
    expect(openUrl).toHaveBeenCalledWith(
      `${server.baseUrl}/v1/exports/${id}/download?token=${downloadToken(1)}`,
    );
    expect(server.auth.requests.find((r) => r.method === 'GET')?.reauth).toBe('reauth-1');
    expect(await screen.findByText(t('dataExport.downloadStarted'))).toBeTruthy();
  });

  it('says when the file is still being built', async () => {
    const server = new FakePrivacyServer();
    const store = createExportStore(memoryKeyValueStore());
    const requestedAt = server.auth.nowMs - EXPORT_DELAY_MS - 1;
    const item = {
      id: '00000000-0000-4000-8000-0000000000bb',
      status: 'building' as const,
      requestedAt,
      availableAt: requestedAt + EXPORT_DELAY_MS,
      expiresAt: null,
    };
    server.exports.push(item);
    await store.save({ ...item, status: 'queued' });

    const { openUrl, user } = await setup({ server, store });
    expect(await screen.findByText(t('dataExport.checkReady'))).toBeTruthy();
    await user.press(screen.getByRole('button', { name: t('dataExport.download') }));
    await confirmWithApple(user);
    expect(await screen.findByText(t('dataExport.stillBuilding'))).toBeTruthy();
    expect(openUrl).not.toHaveBeenCalled();
    expect((await store.load())?.status).toBe('building');
  });

  it('shows the daily limit from Retry-After', async () => {
    const server = new FakePrivacyServer();
    const hourAgo = server.auth.nowMs - 3600_000;
    server.exports.push({
      id: '00000000-0000-4000-8000-0000000000aa',
      status: 'downloaded',
      requestedAt: hourAgo,
      availableAt: hourAgo + EXPORT_DELAY_MS,
      expiresAt: null,
    });
    const { user } = await setup({ server });
    await user.press(await screen.findByRole('button', { name: t('dataExport.request') }));
    await confirmWithApple(user);
    expect(await screen.findByText(t('dataExport.cooldown', { hours: 23 }))).toBeTruthy();
    expect(server.exports).toHaveLength(1);
  });

  it('remembers only the id and the times, never a download path', async () => {
    const kv = memoryKeyValueStore();
    const store = createExportStore(kv);
    await store.save({
      id: '00000000-0000-4000-8000-000000000001',
      status: 'ready',
      requestedAt: 1,
      availableAt: 2,
      expiresAt: 3,
      downloadPath: `/v1/exports/x/download?token=${'a'.repeat(20)}`,
    });
    expect(kv.items.get(EXPORT_KEY)).not.toContain('download');
    kv.items.set(EXPORT_KEY, '{"id":"nope"}');
    expect(await store.load()).toBeNull();
  });
});
