/**
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import type { SkImage } from '@shopify/react-native-skia';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { screen, userEvent, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { FakeBoardsServer } from '../../../test/fakeBoardsServer';
import { ME } from '../../../test/fakeSocialServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { renderThemed } from '../../../test/render';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import type { KeyValueStore } from '../../drafts/draftStore';
import { createBoardsApi } from '../boardsApi';
import { BoardScreen } from '../BoardScreen';
import { createBoardStore } from '../boardStore';
import { createSyncedBoardRepository } from '../boardSync';
import { createTextureCache } from '../textureCache';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const RENDER_TIMEOUT_MS = 30_000;
const OTHER = '00000000-0000-4000-8000-0000000000bb';
const SHARED = '0192f0cc-0000-7000-8000-000000000001';

function memoryKv(): KeyValueStore {
  const data = new Map<string, string>();
  return {
    getItem: async (k) => data.get(k) ?? null,
    setItem: async (k, v) => void data.set(k, v),
    deleteItem: async (k) => void data.delete(k),
  };
}

/** Reads of the shared board, with or without `since_version`. */
const boardReads = (server: FakeBoardsServer) =>
  server.requests.filter((r) => r.method === 'GET' && r.path.startsWith(`/v1/boards/${SHARED}`))
    .length;

async function setup() {
  const server = new FakeBoardsServer();
  server.addBoard(SHARED, OTHER, [ME], 'Trip');
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
  await repo.refreshList();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const textures = createTextureCache<SkImage>({ load: async () => null });
  await renderThemed(
    <QueryClientProvider client={queryClient}>
      <BoardScreen
        repository={repo}
        boardId={SHARED}
        textures={textures}
        pinnableCards={async () => []}
        onClose={jest.fn()}
      />
    </QueryClientProvider>,
  );
  return { server, repo };
}

describe('BoardScreen sync', () => {
  it(
    'says so when I was removed from the board, and drops it',
    async () => {
      const { server, repo } = await setup();
      await waitFor(() => expect(boardReads(server)).toBeGreaterThan(0));
      server.boards.get(SHARED)!.members.delete(ME);
      await userEvent.setup().press(screen.getByRole('button', { name: t('boards.refresh') }));
      expect(await screen.findByText(t('boards.sync.removed'))).toBeTruthy();
      expect(await repo.get(SHARED)).toBeNull();
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'keeps working offline and shows how many changes wait',
    async () => {
      const { server, repo } = await setup();
      await waitFor(() => expect(boardReads(server)).toBeGreaterThan(0));
      server.offline = true;
      const user = userEvent.setup();
      await user.press(screen.getByRole('button', { name: t('boards.decorate.open') }));
      await user.press(screen.getByRole('button', { name: t('effects.deco.star.a11y') }));
      await waitFor(() => expect(repo.pendingCount(SHARED)).toBe(1));
      await user.press(screen.getByRole('button', { name: t('boards.refresh') }));
      expect(await screen.findByText(t('boards.sync.offline', { count: 1 }))).toBeTruthy();

      server.offline = false;
      await user.press(screen.getByRole('button', { name: t('boards.refresh') }));
      await waitFor(() => expect(server.boards.get(SHARED)!.pins.size).toBe(1));
      await waitFor(() =>
        expect(screen.queryByText(t('boards.sync.offline', { count: 1 }))).toBeNull(),
      );
    },
    RENDER_TIMEOUT_MS,
  );
});
