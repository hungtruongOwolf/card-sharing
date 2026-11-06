import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { userEvent } from '@testing-library/react-native';
import type { ReactElement } from 'react';

import type { ColorScheme } from '@card-sharing/tokens';

import { AuthProvider } from '../features/auth/AuthProvider';
import { createAuthClient } from '../features/auth/authClient';
import { sessionFromTokenPair } from '../features/auth/session';
import { createFriendsApi, type FriendsApi } from '../features/friends/friendsApi';
import { createCardsApi, type CardsApi } from '../features/sharing/cardsApi';
import { FakeCardsServer } from './fakeCardsServer';
import { memoryStore } from './onboardingHarness';
import { renderThemed } from './render';

export interface CardsApis {
  cards: CardsApi;
  friends: FriendsApi;
}

/**
 * Renders a card screen against {@link FakeCardsServer} through the real auth client and API modules.
 * `prepare` seeds the fake before the first request.
 */
export async function renderCards(
  screen: (apis: CardsApis) => ReactElement,
  {
    scheme = 'light',
    reducedMotion = false,
    deviceNow,
    prepare,
    queryClient = new QueryClient({
      // No garbage-collection timers: they would keep Jest alive for minutes after the last test.
      defaultOptions: {
        queries: { retry: false, gcTime: Infinity },
        mutations: { retry: 0, gcTime: Infinity },
      },
    }),
  }: {
    scheme?: ColorScheme;
    reducedMotion?: boolean;
    /** The device clock the card API puts signed-URL expiries on (default: the server clock). */
    deviceNow?: (server: FakeCardsServer) => () => number;
    prepare?: (server: FakeCardsServer) => void;
    queryClient?: QueryClient;
  } = {},
) {
  const server = new FakeCardsServer();
  prepare?.(server);
  const store = memoryStore(sessionFromTokenPair(server.auth.issue()));
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store,
    fetch: server.fetch,
    now: server.now,
  });
  const apis: CardsApis = {
    cards: createCardsApi(client.fetch, { now: deviceNow?.(server) ?? server.now }),
    friends: createFriendsApi(client.fetch),
  };
  const user = userEvent.setup();
  const rendered = await renderThemed(
    <QueryClientProvider client={queryClient}>
      <AuthProvider client={client}>{screen(apis)}</AuthProvider>
    </QueryClientProvider>,
    { scheme, reducedMotion },
  );
  return { server, apis, user, client, queryClient, rendered };
}
