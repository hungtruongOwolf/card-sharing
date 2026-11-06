/**
 * Shared boards in the app (T-108): membership calls against the fake server, the invitations inbox, the members
 * sheet, the one-time disclosure and which pins I may change.
 * @jest-environment <rootDir>/jest.skia-environment.js
 */
import type { SkImage } from '@shopify/react-native-skia';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, screen, userEvent, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { cardItem } from '../../../test/boardFixtures';
import { FakeBoardsServer } from '../../../test/fakeBoardsServer';
import { ME } from '../../../test/fakeSocialServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { renderThemed } from '../../../test/render';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import type { KeyValueStore } from '../../drafts/draftStore';
import { createBoardMembersApi, InviteError } from '../boardMembersApi';
import { canEditPin, type BoardPin } from '../boardModel';
import { createBoardsApi } from '../boardsApi';
import { BoardScreen, type BoardScreenProps } from '../BoardScreen';
import { BoardsScreen } from '../BoardsScreen';
import { createBoardStore } from '../boardStore';
import { createSyncedBoardRepository } from '../boardSync';
import { memoryPinDisclosure } from '../pinDisclosure';
import { memoryPublicLinkPrefs } from '../publicLinkPrefs';
import type { PublicLinksApi } from '../publicLinksApi';
import { createTextureCache } from '../textureCache';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const RENDER_TIMEOUT_MS = 30_000;
const OTHER = '00000000-0000-4000-8000-0000000000bb';
const THIRD = '00000000-0000-4000-8000-0000000000cc';
const MINE = '0192f0cc-0000-7000-8000-000000000001';
const THEIRS = '0192f0cc-0000-7000-8000-000000000002';

function memoryKv(): KeyValueStore {
  const data = new Map<string, string>();
  return {
    getItem: async (k) => data.get(k) ?? null,
    setItem: async (k, v) => void data.set(k, v),
    deleteItem: async (k) => void data.delete(k),
  };
}

function setup() {
  const server = new FakeBoardsServer();
  server.names.set(OTHER, 'Mai').set(THIRD, 'Linh').set(ME, 'Me');
  server.friends.add(OTHER).add(THIRD);
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.auth.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  const repo = createSyncedBoardRepository({
    api: createBoardsApi(client.fetch),
    store: createBoardStore(memoryKv()),
  });
  return { server, repo, members: createBoardMembersApi(client.fetch) };
}

/** A `CardView` body of my own card as the server returns it on a board read. */
const cardView = (id: string) => ({
  id,
  sender: { id: ME, display_name: 'Me' },
  caption: null,
  alt_text: null,
  style: { finish: null },
  media: {
    render_url: `https://cdn.test/${id}.png`,
    mask_url: null,
    expires_at: '2030-01-01T00:00:00Z',
  },
  sent_at: '2026-11-05T00:00:00Z',
  is_mine: true,
  my_reaction: null,
});

const queryClient = () => new QueryClient({ defaultOptions: { queries: { retry: false } } });
const textures = () => createTextureCache<SkImage>({ load: async () => null });

describe('board membership api', () => {
  it('invites only with an idempotency key, refuses duplicates, and removes a member with their pins', async () => {
    const { server, members } = setup();
    server.addBoard(MINE, ME, [ME], 'Trip');
    await members.invite(MINE, OTHER, 'key-1');
    expect(server.requests.at(-1)).toMatchObject({
      idempotencyKey: 'key-1',
      body: { user_id: OTHER },
    });
    await expect(members.invite(MINE, OTHER, 'key-2')).rejects.toEqual(
      new InviteError('already_invited'),
    );
    const [sent] = await members.sentInvitations(MINE);
    expect(sent).toMatchObject({ inviteeId: OTHER, inviteeName: 'Mai' });
    await members.cancelInvitation(sent!.id);
    expect(await members.sentInvitations(MINE)).toEqual([]);

    const board = server.boards.get(MINE)!;
    board.members.add(THIRD);
    board.pins.set('p1', {
      id: 'p1',
      createdBy: THIRD,
      cardId: null,
      decoration: { id: 'deco.star', version: 1, params: {} },
      x: 0.5,
      y: 0.5,
      scale: 1,
      rotation: 0,
      zKey: 'a0',
      version: 1,
      changedAt: 0,
      removed: false,
    });
    await members.removeMember(MINE, THIRD);
    expect(board.members.has(THIRD)).toBe(false);
    expect(board.pins.get('p1')!.removed).toBe(true);
  });

  it('lets a collaborator see members with names only and leave', async () => {
    const { server, members } = setup();
    server.addBoard(THEIRS, OTHER, [ME]);
    expect(await members.members(THEIRS)).toEqual([
      { id: OTHER, displayName: 'Mai', role: 'owner', isMe: false },
      { id: ME, displayName: 'Me', role: 'collaborator', isMe: true },
    ]);
    await members.removeMember(THEIRS, ME);
    await expect(members.members(THEIRS)).rejects.toMatchObject({ kind: 'not_found' });
  });
});

describe('pin permissions (AC-19.2, AC-19.7)', () => {
  const pin = (over: Partial<BoardPin>): BoardPin => ({
    id: 'p',
    x: 0,
    y: 0,
    scale: 1,
    rotation: 0,
    zKey: 'a0',
    version: 1,
    content: { kind: 'card', card: cardItem() },
    ...over,
  });
  it('never lets anyone change a private tile, and a collaborator only their own pins', () => {
    expect(canEditPin({ role: 'owner' }, pin({ content: { kind: 'private_tile' } }))).toBe(false);
    expect(canEditPin({ role: 'owner' }, pin({ mine: false }))).toBe(true);
    expect(canEditPin({ role: 'collaborator' }, pin({ mine: false }))).toBe(false);
    expect(canEditPin({ role: 'collaborator' }, pin({ mine: true }))).toBe(true);
    expect(canEditPin({}, pin({}))).toBe(true);
  });
});

describe('invitations inbox', () => {
  it(
    'joins a board only after I accept, then opens it',
    async () => {
      const { server, repo, members } = setup();
      server.addBoard(THEIRS, OTHER, [], 'Picnic');
      server.inviteMe(THEIRS);
      const onOpen = jest.fn();
      await renderThemed(
        <QueryClientProvider client={queryClient()}>
          <BoardsScreen repository={repo} invitations={members} onOpen={onOpen} />
        </QueryClientProvider>,
      );
      expect(
        await screen.findByText(
          t('boards.invitations.item', { owner: 'Mai', title: 'Picnic', count: 1 }),
        ),
      ).toBeTruthy();
      expect(server.boards.get(THEIRS)!.members.has(ME)).toBe(false);
      await userEvent
        .setup()
        .press(screen.getByRole('button', { name: t('boards.invitations.accept') }));
      await waitFor(() => expect(onOpen).toHaveBeenCalledWith(THEIRS));
      expect(server.boards.get(THEIRS)!.members.has(ME)).toBe(true);
      expect((await repo.list()).map((b) => b.id)).toContain(THEIRS);
    },
    RENDER_TIMEOUT_MS,
  );
});

describe('shared board screen', () => {
  async function open(boardId: string, disclosure = memoryPinDisclosure()) {
    const ctx = setup();
    return { ...ctx, disclosure };
  }

  async function render(
    ctx: Awaited<ReturnType<typeof open>>,
    boardId: string,
    cards = [cardItem({ isMine: true })],
    publicLinks?: BoardScreenProps['publicLinks'],
    onReport?: BoardScreenProps['onReport'],
  ) {
    await ctx.repo.refreshList();
    await renderThemed(
      <QueryClientProvider client={queryClient()}>
        <BoardScreen
          repository={ctx.repo}
          boardId={boardId}
          textures={textures()}
          pinnableCards={async () => cards}
          members={ctx.members}
          friends={async () => [
            { id: OTHER, displayName: 'Mai', handle: null },
            { id: THIRD, displayName: 'Linh', handle: null },
          ]}
          disclosure={ctx.disclosure}
          publicLinks={publicLinks}
          onReport={onReport}
          onClose={jest.fn()}
        />
      </QueryClientProvider>,
      { gestureRoot: true },
    );
    await screen.findByRole('button', { name: t('boards.members.open') });
  }

  it(
    'explains once that everyone on a shared board can see my card (AC-19.5)',
    async () => {
      const ctx = await open(THEIRS);
      ctx.server.addBoard(THEIRS, OTHER, [ME]);
      const mine = cardItem({ isMine: true });
      ctx.server.cards.set(mine.id, { view: cardView(mine.id), visibleTo: new Set([ME]) });
      await render(ctx, THEIRS, [mine]);
      const user = userEvent.setup();
      const pinMine = async () => {
        await user.press(screen.getByRole('button', { name: t('boards.add') }));
        await user.press(await screen.findByRole('button', { name: t('boards.picker.card.mine') }));
      };
      await pinMine();
      expect(await screen.findByText(t('boards.disclosure.body'))).toBeTruthy();
      await user.press(screen.getByRole('button', { name: t('boards.disclosure.confirm') }));
      expect(await ctx.disclosure.seen()).toBe(true);
      await waitFor(async () => expect((await ctx.repo.get(THEIRS))!.pins).toHaveLength(1));

      // The second time it is not shown again.
      await user.press(screen.getByRole('button', { name: t('common.dismiss') }));
      await pinMine();
      await waitFor(async () => expect((await ctx.repo.get(THEIRS))!.pins).toHaveLength(2));
      expect(screen.queryByText(t('boards.disclosure.body'))).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'offers Report to a collaborator only, never on my own board (T-141)',
    async () => {
      const onReport = jest.fn();
      const ctx = await open(THEIRS);
      ctx.server.addBoard(THEIRS, OTHER, [ME]);
      await render(ctx, THEIRS, undefined, undefined, onReport);
      fireEvent.press(screen.getByRole('button', { name: t('report.open.board') }));
      expect(onReport).toHaveBeenCalledWith(THEIRS);
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'never offers Report on a board I own',
    async () => {
      const ctx = await open(MINE);
      ctx.server.addBoard(MINE, ME, [ME, OTHER], 'Trip');
      await render(ctx, MINE, undefined, undefined, jest.fn());
      expect(screen.queryByRole('button', { name: t('report.open.board') })).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'offers Share link only to the owner, and never after the server refused it (T-134, AC-24.6)',
    async () => {
      const api: PublicLinksApi = {
        list: jest.fn(async () => []),
        create: jest.fn(),
        revoke: jest.fn(),
      };
      const ctx = await open(MINE);
      ctx.server.addBoard(MINE, ME, [ME], 'Trip');
      ctx.server.addBoard(THEIRS, OTHER, [ME]);
      await render(ctx, MINE, undefined, { api, prefs: memoryPublicLinkPrefs() });
      expect(await screen.findByRole('button', { name: t('boards.links.open') })).toBeTruthy();
      screen.unmount();
      await render(ctx, THEIRS, undefined, { api, prefs: memoryPublicLinkPrefs() });
      expect(screen.queryByRole('button', { name: t('boards.links.open') })).toBeNull();
      screen.unmount();
      await render(ctx, MINE, undefined, {
        api,
        prefs: memoryPublicLinkPrefs({ disallowed: true }),
      });
      expect(screen.queryByRole('button', { name: t('boards.links.open') })).toBeNull();
      screen.unmount();
      // A minor (policy.public_links.allowed = false from /v1/me, #321) never sees it, even before a first try.
      await render(ctx, MINE, undefined, { api, prefs: memoryPublicLinkPrefs(), allowed: false });
      expect(await screen.findByRole('button', { name: t('boards.backdrop.open') })).toBeTruthy();
      expect(screen.queryByRole('button', { name: t('boards.links.open') })).toBeNull();
      expect(api.create).not.toHaveBeenCalled();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'shows members to the owner, invites a friend and hides friends already invited',
    async () => {
      const ctx = await open(MINE);
      ctx.server.addBoard(MINE, ME, [ME], 'Trip');
      await render(ctx, MINE);
      const user = userEvent.setup();
      await user.press(screen.getByRole('button', { name: t('boards.members.open') }));
      expect(
        await screen.findByText(t('boards.members.owner', { name: t('boards.members.you') })),
      ).toBeTruthy();
      await user.press(screen.getByRole('button', { name: t('boards.members.invite') }));
      await user.press(
        await screen.findByRole('button', { name: t('boards.members.inviteOne', { name: 'Mai' }) }),
      );
      expect(
        await screen.findByRole('button', { name: t('boards.members.cancelInvite') }),
      ).toBeTruthy();
      await waitFor(() =>
        expect(
          screen.queryByRole('button', { name: t('boards.members.inviteOne', { name: 'Mai' }) }),
        ).toBeNull(),
      );
      expect(
        screen.getByRole('button', { name: t('boards.members.inviteOne', { name: 'Linh' }) }),
      ).toBeTruthy();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'keeps private tiles and other members’ pins locked for a collaborator',
    async () => {
      const ctx = await open(THEIRS);
      const board = ctx.server.addBoard(THEIRS, OTHER, [ME]);
      board.version = 2;
      const base = {
        decoration: null,
        scale: 1,
        rotation: 0,
        version: 1,
        changedAt: 1,
        removed: false,
      };
      board.pins.set('0192f0ee-0000-7000-8000-000000000001', {
        ...base,
        id: '0192f0ee-0000-7000-8000-000000000001',
        createdBy: OTHER,
        cardId: '0192f0dd-0000-7000-8000-000000000001',
        x: 0.3,
        y: 0.3,
        zKey: 'a0',
      });
      board.pins.set('0192f0ee-0000-7000-8000-000000000002', {
        ...base,
        id: '0192f0ee-0000-7000-8000-000000000002',
        createdBy: OTHER,
        cardId: null,
        decoration: { id: 'deco.star', version: 1, params: {} },
        x: 0.6,
        y: 0.6,
        zKey: 'a1',
      });
      // The card belongs to someone I am not friends with: a private tile for me.
      ctx.server.cards.set('0192f0dd-0000-7000-8000-000000000001', {
        view: {},
        visibleTo: new Set([OTHER]),
      });
      await render(ctx, THEIRS);
      const tile = await screen.findByLabelText(t('boards.pin.private'));
      expect(tile.props.accessibilityActions).toBeUndefined();
      const star = await screen.findByLabelText(
        t('boards.pin.decoration', { name: t('effects.deco.star.name') }),
      );
      expect(star.props.accessibilityActions).toBeUndefined();
      // A collaborator cannot change the board's backdrop.
      expect(screen.queryByRole('button', { name: t('boards.backdrop.open') })).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );
});
