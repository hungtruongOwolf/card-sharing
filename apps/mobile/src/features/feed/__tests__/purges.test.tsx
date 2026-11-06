import { QueryClient, type InfiniteData } from '@tanstack/react-query';
import { act, screen, waitFor } from '@testing-library/react-native';
import type { AppStateStatus } from 'react-native';

import { renderCards } from '../../../test/cardsHarness';
import { problem } from '../../../test/fakeAuthServer';
import type { FakeCardsServer } from '../../../test/fakeCardsServer';
import { memoryKeyValueStore } from '../../drafts/draftStore';
import type { FeedPage } from '../../sharing/cardsApi';
import { createFeedCache } from '../feedCache';
import { FeedScreen } from '../FeedScreen';
import { PurgeSync } from '../PurgeSync';
import { PURGE_CHECK_INTERVAL_MS, syncPurges } from '../purges';
import { cardQueryKey, FEED_QUERY_KEY } from '../queryKeys';

function seed(server: FakeCardsServer) {
  const mai = server.addUser('Mai');
  const ben = server.addUser('Ben');
  server.friends.add(mai.id);
  server.friends.add(ben.id);
  const fromMai = server.addIncomingCard(mai.id, { caption: 'zoomies' });
  const fromBen = server.addIncomingCard(ben.id, { caption: 'nap time' });
  return { mai, ben, fromMai, fromBen };
}

const feedIds = (queryClient: QueryClient) =>
  queryClient
    .getQueryData<InfiniteData<FeedPage>>(FEED_QUERY_KEY)
    ?.pages.flatMap((page) => page.items.map((item) => item.id));

/** A controllable AppState: `set` fires the change listeners like the OS does. */
function fakeAppState(initial: AppStateStatus = 'active') {
  const listeners = new Set<(state: AppStateStatus) => void>();
  const appState = {
    currentState: initial,
    addEventListener: (_: string, listener: (state: AppStateStatus) => void) => {
      listeners.add(listener);
      return { remove: () => listeners.delete(listener) };
    },
    set(state: AppStateStatus) {
      appState.currentState = state;
      for (const listener of listeners) listener(state);
    },
  };
  return appState;
}

/** A manual interval: `tick()` runs the scheduled check; `active` says whether one is scheduled. */
function manualEvery() {
  const state = { tick: null as (() => void) | null, ms: 0 };
  const every = (tick: () => void, ms: number) => {
    state.tick = tick;
    state.ms = ms;
    return () => {
      state.tick = null;
    };
  };
  return { state, every };
}

const noTicks = manualEvery().every;

async function setup(
  withSync?: ReturnType<typeof fakeAppState>,
  every: ReturnType<typeof manualEvery>['every'] = noTicks,
) {
  const cache = createFeedCache(memoryKeyValueStore());
  let people!: ReturnType<typeof seed>;
  let clock = () => 0;
  const rendered = await renderCards(
    (apis) => (
      <>
        <FeedScreen
          api={apis.cards}
          cache={cache}
          now={() => clock()}
          onCompose={() => undefined}
          onBack={() => undefined}
        />
        {withSync ? (
          <PurgeSync api={apis.cards} cache={cache} appState={withSync as never} every={every} />
        ) : null}
      </>
    ),
    {
      prepare: (server) => {
        clock = server.now;
        people = seed(server);
      },
    },
  );
  await screen.findByText('nap time', {}, { timeout: 5000 });
  // The feed saved its offline copy.
  await waitFor(async () =>
    expect((await cache.load(rendered.server.now()))?.items).toHaveLength(2),
  );
  return { ...rendered, ...people, cache };
}

describe('purge sync (T-83)', () => {
  it('drops an unfriended sender’s cards from the offline copy and the feed on screen', async () => {
    const { server, apis, cache, queryClient, ben, fromMai, fromBen } = await setup();
    server.friends.delete(ben.id);
    let purged: string[] = [];
    await act(async () => {
      purged = await syncPurges({ api: apis.cards, cache, queryClient, now: server.now });
    });
    expect(purged).toEqual([fromBen.id]);
    expect((await cache.load(server.now()))?.items.map((item) => item.id)).toEqual([fromMai.id]);
    expect(feedIds(queryClient)).toEqual([fromMai.id]);
    await waitFor(() => expect(screen.queryByText('nap time')).toBeNull());
    expect(screen.getByText('zoomies')).toBeTruthy();
  });

  it('purges deleted cards and blocked senders, and re-reads an open card so it closes', async () => {
    const { server, apis, cache, queryClient, mai, fromMai, fromBen } = await setup();
    queryClient.setQueryData(cardQueryKey(fromBen.id), { stale: true });
    server.cards.delete(fromBen.id);
    server.blocks.set(mai.id, server.now());
    const invalidate = jest.spyOn(queryClient, 'invalidateQueries');
    await act(async () => {
      await syncPurges({ api: apis.cards, cache, queryClient, now: server.now });
    });
    expect((await cache.load(server.now()))?.items).toEqual([]);
    expect(feedIds(queryClient)).toEqual([]);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: cardQueryKey(fromBen.id) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: cardQueryKey(fromMai.id) });
  });

  it('changes nothing when nothing is purged or the check fails', async () => {
    const { server, apis, cache, queryClient, ben } = await setup();
    await act(async () => {
      expect(await syncPurges({ api: apis.cards, cache, queryClient, now: server.now })).toEqual(
        [],
      );
    });
    server.friends.delete(ben.id);
    server.overrides.set('POST /v1/feed/purges', problem(503, 'TEMPORARILY_UNAVAILABLE'));
    await expect(
      syncPurges({ api: apis.cards, cache, queryClient, now: server.now }),
    ).rejects.toBeTruthy();
    expect((await cache.load(server.now()))?.items).toHaveLength(2);
    expect(feedIds(queryClient)).toHaveLength(2);
  });

  it('sends no request when nothing is cached', async () => {
    const server = { checkPurges: jest.fn() };
    const purged = await syncPurges({
      api: server as never,
      cache: createFeedCache(memoryKeyValueStore()),
      queryClient: new QueryClient(),
    });
    expect(purged).toEqual([]);
    expect(server.checkPurges).not.toHaveBeenCalled();
  });

  it('checks each time the app comes to the foreground, not in the background', async () => {
    const appState = fakeAppState('background');
    const { server, ben } = await setup(appState);
    expect(server.count('POST', '/v1/feed/purges')).toBe(0);
    await act(async () => appState.set('active'));
    await waitFor(() => expect(server.count('POST', '/v1/feed/purges')).toBe(1));
    server.friends.delete(ben.id);
    await act(async () => appState.set('background'));
    expect(server.count('POST', '/v1/feed/purges')).toBe(1);
    await act(async () => appState.set('active'));
    await waitFor(() => expect(screen.queryByText('nap time')).toBeNull());
    expect(server.count('POST', '/v1/feed/purges')).toBe(2);
  });

  it('keeps checking every 30 s while in the foreground, and stops in the background', async () => {
    const appState = fakeAppState('background');
    const interval = manualEvery();
    const { server, ben } = await setup(appState, interval.every);
    expect(interval.state.tick).toBeNull();
    await act(async () => appState.set('active'));
    await waitFor(() => expect(server.count('POST', '/v1/feed/purges')).toBe(1));
    expect(interval.state.ms).toBe(PURGE_CHECK_INTERVAL_MS);
    server.friends.delete(ben.id);
    await act(async () => interval.state.tick!());
    await waitFor(() => expect(screen.queryByText('nap time')).toBeNull());
    expect(server.count('POST', '/v1/feed/purges')).toBe(2);
    await act(async () => appState.set('background'));
    expect(interval.state.tick).toBeNull();
  });
});
