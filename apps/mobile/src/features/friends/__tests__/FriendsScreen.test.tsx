import { screen, waitFor, within } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { problem } from '../../../test/fakeAuthServer';
import type { FakeSocialServer } from '../../../test/fakeSocialServer';
import { SCHEMES } from '../../../test/render';
import { renderSocial } from '../../../test/socialHarness';
import { FriendsScreen } from '../FriendsScreen';

const seed = (server: FakeSocialServer) => {
  const mai = server.addUser('Mai', { handle: 'mai.cat' });
  const ben = server.addUser('Ben');
  server.friends.add(mai.id);
  server.friends.add(ben.id);
  return { mai, ben };
};

async function setup(scheme: 'light' | 'dark' = 'light', extra?: (s: FakeSocialServer) => void) {
  let people!: ReturnType<typeof seed>;
  const rendered = await renderSocial(
    (api) => (
      <FriendsScreen
        api={api}
        onOpenRequests={() => undefined}
        onOpenInvite={() => undefined}
        onBack={() => undefined}
      />
    ),
    {
      scheme,
      prepare: (server) => {
        people = seed(server);
        extra?.(server);
      },
    },
  );
  return { ...rendered, ...people };
}

/** The confirm button inside the sheet (rows have buttons with the same label). */
const sheetButton = (label: string) => {
  const last = screen.getAllByRole('button', { name: label }).at(-1);
  if (!last) throw new Error(`no ${label} button`);
  return last;
};

describe('FriendsScreen', () => {
  it.each(SCHEMES)('lists friends with their handle (%s)', async (scheme) => {
    const { mai, ben } = await setup(scheme);
    const row = await screen.findByTestId(`friend-${mai.id}`);
    expect(within(row).getByText('Mai')).toBeTruthy();
    expect(within(row).getByText('@mai.cat')).toBeTruthy();
    expect(screen.getByTestId(`friend-${ben.id}`)).toBeTruthy();
  });

  it('removes a friend only after the confirm, and cancel sends nothing', async () => {
    const { server, user, mai } = await setup();
    await screen.findByTestId(`friend-${mai.id}`);
    await user.press(
      screen.getByRole('button', { name: t('friends.remove.a11y', { name: 'Mai' }) }),
    );
    expect(screen.getByText(t('friends.remove.body'))).toBeTruthy();
    await user.press(screen.getByRole('button', { name: t('friends.confirm.cancel') }));
    expect(server.count('DELETE', '/v1/friends/')).toBe(0);

    await user.press(
      screen.getByRole('button', { name: t('friends.remove.a11y', { name: 'Mai' }) }),
    );
    await user.press(sheetButton(t('friends.remove')));
    await waitFor(() => expect(screen.queryByTestId(`friend-${mai.id}`)).toBeNull());
    expect(server.friends.has(mai.id)).toBe(false);
    expect(screen.getByText(t('friends.removed', { name: 'Mai' }))).toBeTruthy();
  });

  it('blocks a friend after the confirm', async () => {
    const { server, user, ben } = await setup();
    await screen.findByTestId(`friend-${ben.id}`);
    await user.press(
      screen.getByRole('button', { name: t('friends.block.a11y', { name: 'Ben' }) }),
    );
    expect(screen.getByText(t('friends.block.title', { name: 'Ben' }))).toBeTruthy();
    await user.press(sheetButton(t('friends.block')));
    await waitFor(() => expect(screen.queryByTestId(`friend-${ben.id}`)).toBeNull());
    expect(server.blocks.has(ben.id)).toBe(true);
  });

  it('refreshes when the friend is already gone', async () => {
    const { server, user, mai } = await setup();
    await screen.findByTestId(`friend-${mai.id}`);
    server.friends.delete(mai.id);
    await user.press(
      screen.getByRole('button', { name: t('friends.remove.a11y', { name: 'Mai' }) }),
    );
    await user.press(sheetButton(t('friends.remove')));
    expect(await screen.findByText(t('friends.error.gone'))).toBeTruthy();
    await waitFor(() => expect(screen.queryByTestId(`friend-${mai.id}`)).toBeNull());
  });

  it('shows the empty state and a retry after a failed load', async () => {
    const { user } = await renderSocial(
      (api) => (
        <FriendsScreen
          api={api}
          onOpenRequests={() => undefined}
          onOpenInvite={() => undefined}
          onBack={() => undefined}
        />
      ),
      { prepare: (server) => server.overrides.set('GET /v1/friends', problem(503, 'UNAVAILABLE')) },
    );
    expect(await screen.findByText(t('friends.error.load'))).toBeTruthy();
    await user.press(screen.getByRole('button', { name: t('friends.retry') }));
    expect(await screen.findByText(t('friends.empty'))).toBeTruthy();
  });
});
