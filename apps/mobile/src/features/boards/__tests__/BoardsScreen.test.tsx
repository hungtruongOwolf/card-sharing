import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { screen, userEvent, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import { createMemoryBoardRepository } from '../boardRepository';
import { BoardsScreen } from '../BoardsScreen';

describe('BoardsScreen', () => {
  it('starts a personal board, lists it and opens it', async () => {
    const repository = createMemoryBoardRepository();
    const onOpen = jest.fn();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    await renderThemed(
      <QueryClientProvider client={client}>
        <BoardsScreen repository={repository} onOpen={onOpen} />
      </QueryClientProvider>,
    );
    expect(await screen.findByText(t('boards.list.empty'))).toBeTruthy();
    await userEvent.setup().press(screen.getByRole('button', { name: t('boards.new') }));
    await waitFor(() => expect(onOpen).toHaveBeenCalledTimes(1));
    const [board] = await repository.list();
    expect(onOpen).toHaveBeenCalledWith(board!.id);
    expect(
      await screen.findByRole('button', {
        name: t('boards.list.item', { title: t('boards.defaultTitle'), count: 0 }),
      }),
    ).toBeTruthy();
  });
});
