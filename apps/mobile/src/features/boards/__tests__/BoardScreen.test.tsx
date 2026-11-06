/**
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { Skia } from '@shopify/react-native-skia';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { fireEvent, screen, userEvent, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { cardItem } from '../../../test/boardFixtures';
import { renderThemed } from '../../../test/render';
import { createMemoryBoardRepository } from '../boardRepository';
import { BoardScreen } from '../BoardScreen';
import { createTextureCache } from '../textureCache';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

const RENDER_TIMEOUT_MS = 30_000;

async function setup() {
  const repository = createMemoryBoardRepository();
  const board = await repository.create('Desk');
  const textures = createTextureCache({
    load: async () => {
      const surface = Skia.Surface.Make(16, 16)!;
      surface.getCanvas().clear(Skia.Color('#3366CC'));
      surface.flush();
      return surface.makeImageSnapshot();
    },
  });
  const cards = [
    cardItem({ sender: { id: 'u1', displayName: 'Mai' } }),
    cardItem({ isMine: true }),
    cardItem({ media: null }),
  ];
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  await renderThemed(
    <QueryClientProvider client={client}>
      <BoardScreen
        repository={repository}
        boardId={board.id}
        textures={textures}
        pinnableCards={async () => cards}
        onClose={jest.fn()}
      />
    </QueryClientProvider>,
  );
  await screen.findByText(t('boards.empty'));
  return { repository, board };
}

describe('BoardScreen', () => {
  it(
    'pins a card from the picker, then removes it and undoes the removal',
    async () => {
      const { repository, board } = await setup();
      const user = userEvent.setup();
      await user.press(screen.getByRole('button', { name: t('boards.add') }));
      // A card without a delivered render is not offered.
      expect(await screen.findByLabelText(t('boards.picker.card.mine'))).toBeTruthy();
      expect(screen.getAllByLabelText(t('boards.picker.card', { name: 'Mai' }))).toHaveLength(1);
      await user.press(screen.getByLabelText(t('boards.picker.card', { name: 'Mai' })));

      expect(await screen.findByLabelText(t('boards.pin.card', { name: 'Mai' }))).toBeTruthy();
      await waitFor(async () => expect((await repository.get(board.id))!.pins).toHaveLength(1));

      // The new pin is selected: its toolbar offers remove.
      await user.press(screen.getByRole('button', { name: t('boards.action.remove') }));
      await waitFor(async () => expect((await repository.get(board.id))!.pins).toHaveLength(0));
      await user.press(await screen.findByRole('button', { name: t('common.undo') }));
      await waitFor(async () => expect((await repository.get(board.id))!.pins).toHaveLength(1));
    },
    RENDER_TIMEOUT_MS,
  );

  it(
    'applies screen-reader actions as one op on that pin',
    async () => {
      const { repository, board } = await setup();
      const user = userEvent.setup();
      await user.press(screen.getByRole('button', { name: t('boards.add') }));
      await user.press(await screen.findByLabelText(t('boards.picker.card.mine')));
      const before = (await repository.get(board.id))!.pins[0]!;
      const label = await screen.findByLabelText(t('boards.pin.card.mine'));
      await fireEvent(label, 'accessibilityAction', { nativeEvent: { actionName: 'moveRight' } });
      await waitFor(async () =>
        expect((await repository.get(board.id))!.pins[0]!.x).toBeGreaterThan(before.x),
      );
      await fireEvent(screen.getByLabelText(t('boards.pin.card.mine')), 'accessibilityAction', {
        nativeEvent: { actionName: 'remove' },
      });
      await waitFor(async () => expect((await repository.get(board.id))!.pins).toHaveLength(0));
    },
    RENDER_TIMEOUT_MS,
  );
});
