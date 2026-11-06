import { fireEvent, screen, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { renderCards } from '../../../test/cardsHarness';
import { problem } from '../../../test/fakeAuthServer';
import { FakeCardsServer } from '../../../test/fakeCardsServer';
import { SCHEMES } from '../../../test/render';
import { memoryKeyValueStore } from '../../drafts/draftStore';
import { createFeedCache, type FeedCache } from '../feedCache';
import { FeedScreen } from '../FeedScreen';

const REACTIONS = ['react.heart', 'react.paw', 'react.unknown'];

function seed(server: FakeCardsServer) {
  const mai = server.addUser('Mai');
  const ben = server.addUser('Ben');
  server.friends.add(mai.id);
  server.friends.add(ben.id);
  const newest = server.addIncomingCard(mai.id, { caption: 'zoomies' });
  const older = server.addIncomingCard(ben.id, { caption: 'nap time' });
  return { mai, ben, newest, older };
}

async function setup({
  scheme = 'light' as const,
  cache = createFeedCache(memoryKeyValueStore()),
  extra,
}: {
  scheme?: 'light' | 'dark';
  cache?: FeedCache;
  extra?: (server: FakeCardsServer, people: ReturnType<typeof seed>) => void;
} = {}) {
  let people!: ReturnType<typeof seed>;
  const onCompose = jest.fn();
  let clock = () => 0;
  const rendered = await renderCards(
    (apis) => (
      <FeedScreen
        api={apis.cards}
        cache={cache}
        reactions={REACTIONS}
        now={() => clock()}
        onCompose={onCompose}
        onBack={() => undefined}
      />
    ),
    {
      scheme,
      prepare: (server) => {
        clock = server.now;
        people = seed(server);
        extra?.(server, people);
      },
    },
  );
  return { ...rendered, ...people, cache, onCompose };
}

describe('FeedScreen', () => {
  it.each(SCHEMES)('shows cards newest first with their pictures (%s)', async (scheme) => {
    const { newest, older } = await setup({ scheme });
    const first = await screen.findByTestId(`feed-card-${newest.id}`);
    expect(screen.getByText('zoomies')).toBeTruthy();
    expect(screen.getByText('Mai')).toBeTruthy();
    const ids = screen.getAllByTestId(/^feed-card-/).map((node) => node.props.testID);
    expect(ids).toEqual([`feed-card-${newest.id}`, `feed-card-${older.id}`]);
    expect(first).toBeTruthy();
    expect(screen.getByTestId(`feed-image-${newest.id}`).props.source.uri).toContain(
      'https://media.test/',
    );
  });

  it('pages with the cursor and recovers from a cursor the server refuses', async () => {
    const { server, user } = await setup({
      extra: (s) => {
        s.feedPageSize = 1;
      },
    });
    await screen.findByText('zoomies');
    expect(screen.queryByText('nap time')).toBeNull();
    await user.press(screen.getByRole('button', { name: t('feed.older') }));
    expect(await screen.findByText('nap time')).toBeTruthy();
    expect(server.auth.requests.some((r) => r.path.includes(FakeCardsServer.cursorFor(1)))).toBe(
      true,
    );
    expect(screen.queryByRole('button', { name: t('feed.older') })).toBeNull();
  });

  it('starts over from the newest card when the next cursor is refused', async () => {
    const { server, user } = await setup({ extra: (s) => (s.feedPageSize = 1) });
    await screen.findByText('zoomies');
    server.overrides.set('GET /v1/feed', problem(400, 'VALIDATION_FAILED'));
    await user.press(screen.getByRole('button', { name: t('feed.older') }));
    await waitFor(() => expect(server.count('GET', '/v1/feed')).toBeGreaterThanOrEqual(3));
    expect(await screen.findByText('zoomies')).toBeTruthy();
  });

  it('refetches new signed URLs when a picture fails to load (expired URL)', async () => {
    const { server, newest } = await setup();
    const image = await screen.findByTestId(`feed-image-${newest.id}`);
    const before = server.count('GET', '/v1/feed');
    await fireEvent(image, 'error', { nativeEvent: { error: '403' } });
    await waitFor(() => expect(server.count('GET', '/v1/feed')).toBe(before + 1));
    // Pictures that keep failing do not loop the feed (the fake clock stands still).
    await fireEvent(screen.getByTestId(`feed-image-${newest.id}`), 'error', {});
    await fireEvent(screen.getByTestId(`feed-image-${newest.id}`), 'error', {});
    expect(server.count('GET', '/v1/feed')).toBe(before + 1);
  });

  it('never shows a picture whose URL has already expired', async () => {
    const { newest } = await setup({ extra: (s) => (s.mediaTtlMs = -1000) });
    expect(await screen.findByTestId(`feed-placeholder-${newest.id}`)).toBeTruthy();
    expect(screen.queryByTestId(`feed-image-${newest.id}`)).toBeNull();
  });

  it('drops cards from people who are no longer friends on the next load', async () => {
    const { server, mai, newest, queryClient } = await setup();
    await screen.findByText('zoomies');
    server.friends.delete(mai.id);
    await queryClient.invalidateQueries({ queryKey: ['feed'] });
    await waitFor(() => expect(screen.queryByTestId(`feed-card-${newest.id}`)).toBeNull());
    expect(screen.getByText('nap time')).toBeTruthy();
  });

  it('reacts privately, replaces and removes the reaction', async () => {
    const { server, user, newest } = await setup();
    await screen.findByText('zoomies');
    const heart = () => screen.getAllByRole('button', { name: t('feed.reaction.heart') })[0]!;
    const paw = () => screen.getAllByRole('button', { name: t('feed.reaction.paw') })[0]!;
    // Unknown ids from config are not offered.
    expect(screen.getAllByRole('button', { name: t('feed.reaction.heart') })).toHaveLength(2);
    await user.press(heart());
    await waitFor(() => expect(server.reactions.get(newest.id)).toBe('react.heart'));
    expect(heart().props.accessibilityState).toMatchObject({ selected: true });
    await user.press(paw());
    await waitFor(() => expect(server.reactions.get(newest.id)).toBe('react.paw'));
    await user.press(paw());
    await waitFor(() => expect(server.reactions.has(newest.id)).toBe(false));
    expect(paw().props.accessibilityState).toMatchObject({ selected: false });
  });

  it('undoes the reaction and explains when the card is gone', async () => {
    const { server, user, newest, mai } = await setup();
    await screen.findByText('zoomies');
    server.blocks.set(mai.id, server.now());
    await user.press(screen.getAllByRole('button', { name: t('feed.reaction.heart') })[0]!);
    expect(await screen.findByText(t('feed.error.gone'))).toBeTruthy();
    expect(server.reactions.has(newest.id)).toBe(false);
    await waitFor(() => expect(screen.queryByTestId(`feed-card-${newest.id}`)).toBeNull());
  });

  it('shows the empty state with a way to make a card', async () => {
    const { user, onCompose } = await setup({
      extra: (s) => {
        s.cards.clear();
      },
    });
    expect(await screen.findByTestId('feed-empty')).toBeTruthy();
    await user.press(screen.getByRole('button', { name: t('feed.compose') }));
    expect(onCompose).toHaveBeenCalled();
  });

  it('saves the newest cards for offline use, without any media URL', async () => {
    const kv = memoryKeyValueStore();
    const { server } = await setup({ cache: createFeedCache(kv) });
    await screen.findByText('zoomies');
    await waitFor(() => expect(kv.items.has('cs.feed.cache.v1.meta')).toBe(true));
    const stored = [...kv.items.values()].join(' ');
    expect(stored).toContain('zoomies');
    expect(stored).not.toContain('https://');
    expect((await createFeedCache(kv).load(server.now()))?.items).toHaveLength(2);
  });

  it('shows the saved cards without pictures when the feed cannot load', async () => {
    const kv = memoryKeyValueStore();
    const cache = createFeedCache(kv);
    const saved = new FakeCardsServer();
    const mai = saved.addUser('Mai');
    saved.friends.add(mai.id);
    const card = saved.addIncomingCard(mai.id, { caption: 'saved zoomies' });
    await cache.save(
      [
        {
          id: card.id,
          sender: { id: mai.id, displayName: 'Mai' },
          caption: 'saved zoomies',
          altText: null,
          media: null,
          sentAt: card.sentAt,
          isMine: false,
          status: null,
          hold: null,
          myReaction: null,
        },
      ],
      saved.now(),
    );
    await setup({
      cache,
      extra: (s) => s.overrides.set('GET /v1/feed', problem(503, 'TEMPORARILY_UNAVAILABLE')),
    });
    expect(await screen.findByTestId('feed-offline')).toBeTruthy();
    expect(screen.getByText('saved zoomies')).toBeTruthy();
    expect(screen.getByText(t('feed.offline.picture'))).toBeTruthy();
    expect(screen.queryByTestId(/^feed-image-/)).toBeNull();
  });

  it('shows an error with a retry when offline with nothing saved', async () => {
    const { server, user } = await setup({
      extra: (s) => s.overrides.set('GET /v1/feed', problem(503, 'TEMPORARILY_UNAVAILABLE')),
    });
    expect(await screen.findByTestId('feed-error')).toBeTruthy();
    await user.press(screen.getByRole('button', { name: t('feed.retry') }));
    expect(await screen.findByText('zoomies')).toBeTruthy();
    expect(server.count('GET', '/v1/feed')).toBe(2);
  });
});
