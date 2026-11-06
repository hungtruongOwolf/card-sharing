import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { screen, userEvent, waitFor, within } from '@testing-library/react-native';
import type { ReactElement } from 'react';

import type { ColorScheme } from '@card-sharing/tokens';

import { t } from '../../../i18n';
import { problem } from '../../../test/fakeAuthServer';
import { FakeActivityServer } from '../../../test/fakeActivityServer';
import { ME } from '../../../test/fakeSocialServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { renderThemed, SCHEMES } from '../../../test/render';
import { AuthProvider } from '../../auth/AuthProvider';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { createFriendsApi } from '../../friends/friendsApi';
import { createCardsApi } from '../../sharing/cardsApi';
import { createActivityApi, type ActivityApi } from '../activityApi';
import { ActivityButton } from '../ActivityButton';
import { ActivityScreen } from '../ActivityScreen';

interface Handlers {
  onOpenCard: jest.Mock;
  onOpenRequests: jest.Mock;
  onOpenFriends: jest.Mock;
}

async function setup({
  scheme = 'light',
  prepare,
  ui,
}: {
  scheme?: ColorScheme;
  prepare?: (server: FakeActivityServer) => void;
  ui?: (api: ActivityApi) => ReactElement;
} = {}) {
  const server = new FakeActivityServer();
  prepare?.(server);
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.auth.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Infinity },
      mutations: { retry: 0, gcTime: Infinity },
    },
  });
  const handlers: Handlers = {
    onOpenCard: jest.fn(),
    onOpenRequests: jest.fn(),
    onOpenFriends: jest.fn(),
  };
  const activity = createActivityApi(client.fetch);
  await renderThemed(
    <QueryClientProvider client={queryClient}>
      <AuthProvider client={client}>
        {ui ? (
          ui(activity)
        ) : (
          <ActivityScreen
            activity={activity}
            cards={createCardsApi(client.fetch, { now: server.now })}
            friends={createFriendsApi(client.fetch)}
            onBack={() => undefined}
            now={server.now}
            {...handlers}
          />
        )}
      </AuthProvider>
    </QueryClientProvider>,
    { scheme },
  );
  return { server, queryClient, handlers, user: userEvent.setup() };
}

/** Mai (a friend) sent me a card with a caption and reacted to mine; Ben asked to be friends; Lan accepted. */
function seed(server: FakeActivityServer) {
  const mai = server.addUser('Mai');
  const ben = server.addUser('Ben');
  const lan = server.addUser('Lan');
  server.friends.add(mai.id);
  server.friends.add(lan.id);
  const incoming = server.addIncomingCard(mai.id, { caption: 'nap time' });
  const mine = server.addIncomingCard(ME, { recipients: [mai.id] });
  const held = server.addIncomingCard(ME, { status: 'held_review', recipients: [mai.id] });
  const request = server.addRequest(ben, 'incoming');
  const items = {
    cardNew: server.addActivity('card.new', incoming.id, mai.id),
    reaction: server.addActivity('reaction.new', mine.id, mai.id),
    held: server.addActivity('card.held', held.id),
    request: server.addActivity('friend.request', request.id, ben.id),
    accepted: server.addActivity('friend.accepted', lan.id, lan.id),
  };
  return { mai, ben, lan, incoming, mine, held, request, items };
}

describe('ActivityScreen', () => {
  it.each(SCHEMES)(
    'shows every kind of event with names from their own routes (%s)',
    async (scheme) => {
      let s!: ReturnType<typeof seed>;
      await setup({ scheme, prepare: (server) => (s = seed(server)) });
      const cardRow = await screen.findByTestId(`activity-${s.items.cardNew.id}`);
      expect(await within(cardRow).findByText(t('activity.cardNew', { name: 'Mai' }))).toBeTruthy();
      expect(within(cardRow).getByText('nap time')).toBeTruthy();
      expect(await screen.findByText(t('activity.reaction', { name: 'Mai' }))).toBeTruthy();
      expect(screen.getByText(t('activity.cardHeld'))).toBeTruthy();
      expect(await screen.findByText(t('activity.friendRequest', { name: 'Ben' }))).toBeTruthy();
      expect(await screen.findByText(t('activity.friendAccepted', { name: 'Lan' }))).toBeTruthy();
    },
  );

  it('marks everything up to the newest item read when viewed', async () => {
    let s!: ReturnType<typeof seed>;
    const { server } = await setup({ prepare: (srv) => (s = seed(srv)) });
    await screen.findByTestId(`activity-${s.items.accepted.id}`);
    await waitFor(() => expect(server.count('POST', '/v1/activity/read')).toBe(1));
    expect(server.auth.requests.find((r) => r.path === '/v1/activity/read')?.body).toEqual({
      up_to: s.items.accepted.id,
    });
    expect(server.readWatermark).toBe(s.items.accepted.seq);
    // This visit still shows what was new.
    expect(
      screen.getByTestId(`activity-new-${s.items.cardNew.id}`, { includeHiddenElements: true }),
    ).toBeTruthy();
  });

  it('sends no mark-read when nothing is unread', async () => {
    const { server } = await setup({
      prepare: (srv) => {
        const s = seed(srv);
        srv.readWatermark = s.items.accepted.seq;
      },
    });
    await screen.findByText(t('activity.cardHeld'));
    expect(server.count('POST', '/v1/activity/read')).toBe(0);
  });

  it('opens the card, the requests or the friends list', async () => {
    let s!: ReturnType<typeof seed>;
    const { handlers, user } = await setup({ prepare: (srv) => (s = seed(srv)) });
    await user.press(await screen.findByTestId(`activity-${s.items.cardNew.id}`));
    expect(handlers.onOpenCard).toHaveBeenCalledWith(s.incoming.id);
    await user.press(await screen.findByTestId(`activity-${s.items.request.id}`));
    expect(handlers.onOpenRequests).toHaveBeenCalled();
    await user.press(await screen.findByTestId(`activity-${s.items.accepted.id}`));
    expect(handlers.onOpenFriends).toHaveBeenCalled();
  });

  it('drops items whose card or person the caller may no longer see on re-read', async () => {
    let s!: ReturnType<typeof seed>;
    const { server, queryClient } = await setup({ prepare: (srv) => (s = seed(srv)) });
    await screen.findByText(t('activity.cardNew', { name: 'Mai' }));
    await screen.findByText(t('activity.friendAccepted', { name: 'Lan' }));
    // Unfriend Mai (drops her card and her reaction), Lan blocked, Ben's request declined elsewhere.
    server.friends.delete(s.mai.id);
    server.blocks.set(s.lan.id, server.now());
    s.request.status = 'declined';
    await queryClient.invalidateQueries();
    await waitFor(() => expect(screen.queryByTestId(`activity-${s.items.cardNew.id}`)).toBeNull());
    expect(screen.queryByTestId(`activity-${s.items.reaction.id}`)).toBeNull();
    expect(screen.queryByTestId(`activity-${s.items.request.id}`)).toBeNull();
    expect(screen.queryByTestId(`activity-${s.items.accepted.id}`)).toBeNull();
    expect(screen.getByTestId(`activity-${s.items.held.id}`)).toBeTruthy();
  });

  it('hides a card item whose card read answers 404 even before the list is re-read', async () => {
    let s!: ReturnType<typeof seed>;
    const { server } = await setup({
      prepare: (srv) => {
        s = seed(srv);
        // The list still has the item, but the card is gone (deleted between the two reads).
        srv.overrides.set(`GET /v1/cards/${s.incoming.id}`, problem(404, 'NOT_FOUND'));
      },
    });
    await screen.findByText(t('activity.cardHeld'));
    await waitFor(() => expect(server.count('GET', `/v1/cards/${s.incoming.id}`)).toBe(1));
    await waitFor(() => expect(screen.queryByTestId(`activity-${s.items.cardNew.id}`)).toBeNull());
  });

  it('shows the empty state and a retry on failure', async () => {
    const { server, user } = await setup({
      prepare: (srv) =>
        srv.overrides.set('GET /v1/activity', problem(503, 'TEMPORARILY_UNAVAILABLE')),
    });
    await screen.findByTestId('activity-error');
    await user.press(screen.getByRole('button', { name: t('activity.retry') }));
    await screen.findByTestId('activity-empty');
    expect(server.count('POST', '/v1/activity/read')).toBe(0);
  });

  it('loads older items page by page', async () => {
    const { user } = await setup({
      prepare: (srv) => {
        srv.activityPageSize = 2;
        seed(srv);
      },
    });
    await screen.findByText(t('activity.friendAccepted', { name: 'Lan' }));
    expect(screen.queryByText(t('activity.cardHeld'))).toBeNull();
    await user.press(screen.getByRole('button', { name: t('activity.older') }));
    await screen.findByText(t('activity.cardHeld'));
  });
});

describe('ActivityButton', () => {
  it('shows the unread count as a badge', async () => {
    await setup({
      prepare: (srv) => seed(srv),
      ui: (api) => <ActivityButton api={api} onPress={() => undefined} />,
    });
    expect(await screen.findByText('5', { includeHiddenElements: true })).toBeTruthy();
    expect(
      screen.getByRole('button', { name: t('activity.open.a11y', { count: 5 }) }),
    ).toBeTruthy();
  });

  it('shows 99+ from 100 unread', async () => {
    await setup({
      prepare: (srv) => {
        const lan = srv.addUser('Lan');
        srv.friends.add(lan.id);
        for (let i = 0; i < 120; i += 1) srv.addActivity('friend.accepted', lan.id, lan.id);
      },
      ui: (api) => <ActivityButton api={api} onPress={() => undefined} />,
    });
    expect(await screen.findByText('99+', { includeHiddenElements: true })).toBeTruthy();
    expect(screen.getByRole('button', { name: t('activity.open.a11y.many') })).toBeTruthy();
  });

  it('shows no badge when everything is read', async () => {
    await setup({ ui: (api) => <ActivityButton api={api} onPress={() => undefined} /> });
    await screen.findByRole('button', { name: t('activity.open') });
    await waitFor(() =>
      expect(screen.queryByTestId('activity-badge', { includeHiddenElements: true })).toBeNull(),
    );
  });
});
