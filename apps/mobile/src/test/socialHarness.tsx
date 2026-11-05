import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { userEvent } from '@testing-library/react-native';
import type { ReactElement } from 'react';

import type { ColorScheme } from '@card-sharing/tokens';

import { AuthProvider } from '../features/auth/AuthProvider';
import { createAuthClient } from '../features/auth/authClient';
import { sessionFromTokenPair } from '../features/auth/session';
import { createFriendsApi, type FriendsApi } from '../features/friends/friendsApi';
import { FakeSocialServer } from './fakeSocialServer';
import { memoryStore } from './onboardingHarness';
import { renderThemed } from './render';

/**
 * Renders a friends screen against {@link FakeSocialServer} through the real auth client and API module.
 * `prepare` seeds the fake before the first request.
 */
export async function renderSocial(
  screen: (api: FriendsApi) => ReactElement,
  {
    scheme = 'light',
    prepare,
  }: { scheme?: ColorScheme; prepare?: (server: FakeSocialServer) => void } = {},
) {
  const server = new FakeSocialServer();
  prepare?.(server);
  const store = memoryStore(sessionFromTokenPair(server.auth.issue()));
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store,
    fetch: server.fetch,
    now: server.now,
  });
  const api = createFriendsApi(client.fetch);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: 0 } },
  });
  const user = userEvent.setup();
  await renderThemed(
    <QueryClientProvider client={queryClient}>
      <AuthProvider client={client}>{screen(api)}</AuthProvider>
    </QueryClientProvider>,
    { scheme },
  );
  return { server, api, user, client };
}
