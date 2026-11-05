import { screen, waitFor, within } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { problem } from '../../../test/fakeAuthServer';
import type { FakeSocialServer } from '../../../test/fakeSocialServer';
import { SCHEMES } from '../../../test/render';
import { renderSocial } from '../../../test/socialHarness';
import { RequestsScreen } from '../RequestsScreen';

const seed = (server: FakeSocialServer) => {
  const ana = server.addUser('Ana', { adultNoMutualFriends: true });
  const bo = server.addUser('Bo', { handle: 'bo' });
  const cy = server.addUser('Cy');
  return {
    fromAna: server.addRequest(ana, 'incoming'),
    fromBo: server.addRequest(bo, 'incoming'),
    toCy: server.addRequest(cy, 'outgoing'),
    ana,
    bo,
  };
};

async function setup(scheme: 'light' | 'dark' = 'light', extra?: (s: FakeSocialServer) => void) {
  let seeded!: ReturnType<typeof seed>;
  const rendered = await renderSocial(
    (api) => <RequestsScreen api={api} onBack={() => undefined} />,
    {
      scheme,
      prepare: (server) => {
        seeded = seed(server);
        extra?.(server);
      },
    },
  );
  return { ...rendered, ...seeded };
}

describe('RequestsScreen', () => {
  it.each(SCHEMES)(
    'shows incoming with the minor hint only where the server set it, and outgoing as pending (%s)',
    async (scheme) => {
      const { fromAna, fromBo, toCy } = await setup(scheme);
      const ana = await screen.findByTestId(`incoming-${fromAna.id}`);
      expect(within(ana).getByText(t('friends.hint.adultNoMutual'))).toBeTruthy();
      expect(within(ana).getByLabelText(t('friends.hint.adultNoMutual.a11y'))).toBeTruthy();
      const bo = screen.getByTestId(`incoming-${fromBo.id}`);
      expect(within(bo).queryByTestId('hint-adult-no-mutual')).toBeNull();
      const cy = await screen.findByTestId(`outgoing-${toCy.id}`);
      expect(within(cy).getByText(t('friends.requests.pending'))).toBeTruthy();
    },
  );

  it('accepts a request and makes a friend', async () => {
    const { server, user, fromBo, bo } = await setup();
    await screen.findByTestId(`incoming-${fromBo.id}`);
    await user.press(
      screen.getByRole('button', { name: t('friends.accept.a11y', { name: 'Bo' }) }),
    );
    await waitFor(() => expect(screen.queryByTestId(`incoming-${fromBo.id}`)).toBeNull());
    expect(server.friends.has(bo.id)).toBe(true);
    expect(screen.getByText(t('friends.accepted', { name: 'Bo' }))).toBeTruthy();
  });

  it('keeps the request and says the friend list is full', async () => {
    const { server, user, fromBo } = await setup('light', (s) => {
      for (let i = 0; i < 20; i += 1) s.friends.add(s.addUser(`F${i}`).id);
    });
    await screen.findByTestId(`incoming-${fromBo.id}`);
    await user.press(
      screen.getByRole('button', { name: t('friends.accept.a11y', { name: 'Bo' }) }),
    );
    expect(await screen.findByText(t('friends.error.friendLimit'))).toBeTruthy();
    expect(screen.getByTestId(`incoming-${fromBo.id}`)).toBeTruthy();
    expect(server.friends.size).toBe(20);
  });

  it('declines and cancels', async () => {
    const { server, user, fromAna, toCy } = await setup();
    await screen.findByTestId(`incoming-${fromAna.id}`);
    await user.press(
      screen.getByRole('button', { name: t('friends.decline.a11y', { name: 'Ana' }) }),
    );
    await waitFor(() => expect(screen.queryByTestId(`incoming-${fromAna.id}`)).toBeNull());
    await user.press(
      screen.getByRole('button', { name: t('friends.cancel.a11y', { name: 'Cy' }) }),
    );
    await waitFor(() => expect(screen.queryByTestId(`outgoing-${toCy.id}`)).toBeNull());
    expect(server.requests.find((r) => r.id === toCy.id)?.status).toBe('cancelled');
  });

  it('blocks a sender only after the confirm', async () => {
    const { server, user, fromAna, ana } = await setup();
    await screen.findByTestId(`incoming-${fromAna.id}`);
    await user.press(
      screen.getByRole('button', { name: t('friends.block.a11y', { name: 'Ana' }) }),
    );
    expect(server.count('POST', '/v1/blocks')).toBe(0);
    const confirm = screen.getAllByRole('button', { name: t('friends.block') }).at(-1);
    if (!confirm) throw new Error('no confirm');
    await user.press(confirm);
    await waitFor(() => expect(screen.queryByTestId(`incoming-${fromAna.id}`)).toBeNull());
    expect(server.blocks.has(ana.id)).toBe(true);
  });

  it('says a request is no longer open and refreshes', async () => {
    const { server, user, fromBo } = await setup();
    await screen.findByTestId(`incoming-${fromBo.id}`);
    server.overrides.set(`POST /v1/friend-requests/${fromBo.id}/accept`, problem(404, 'NOT_FOUND'));
    await user.press(
      screen.getByRole('button', { name: t('friends.accept.a11y', { name: 'Bo' }) }),
    );
    expect(await screen.findByText(t('friends.error.requestGone'))).toBeTruthy();
  });
});
