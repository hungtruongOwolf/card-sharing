import { screen, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { problem } from '../../../test/fakeAuthServer';
import { ME, type FakeSocialServer } from '../../../test/fakeSocialServer';
import { SCHEMES } from '../../../test/render';
import { renderSocial } from '../../../test/socialHarness';
import { FindFriendScreen, searchableHandle } from '../FindFriendScreen';

const seed = (s: FakeSocialServer) => {
  s.addUser('Mai', { handle: 'mai.cat' });
  s.addUser('Hidden', { handle: 'hidden', findable: false });
  const blocked = s.addUser('Blocked', { handle: 'blocked' });
  s.blocks.set(blocked.id, s.now());
};

const render = (scheme: 'light' | 'dark' = 'light', extra?: (s: FakeSocialServer) => void) =>
  renderSocial((api) => <FindFriendScreen api={api} onBack={() => undefined} />, {
    scheme,
    prepare: (s) => {
      seed(s);
      extra?.(s);
    },
  });

const field = () => screen.getByLabelText(t('find.field.label'));
const searchButton = () => screen.getByRole('button', { name: t('find.search') });

describe('FindFriendScreen', () => {
  it('sends nothing while typing, only on submit', async () => {
    const { server, user } = await render();
    await user.type(field(), '@mai');
    expect(server.count('POST', '/v1/users/search')).toBe(0);
    await user.type(field(), '.cat');
    expect(server.count('POST', '/v1/users/search')).toBe(0);
    await user.press(searchButton());
    await screen.findByTestId('find-result');
    expect(server.count('POST', '/v1/users/search')).toBe(1);
    expect(server.auth.requests.at(-1)?.body).toEqual({ handle: 'mai.cat' });
  });

  it.each(SCHEMES)(
    'finds a person and sends a request with the opaque ref (%s)',
    async (scheme) => {
      const { server, user } = await render(scheme);
      await user.type(field(), '  @Mai.Cat ');
      await user.press(searchButton());
      expect(await screen.findByText('Mai')).toBeTruthy();
      expect(screen.queryByText(/mai\.cat/)).toBeNull(); // the result shows the display name only
      await user.press(screen.getByRole('button', { name: t('find.send.a11y', { name: 'Mai' }) }));
      expect(await screen.findByTestId('find-sent')).toBeTruthy();
      const sent = server.auth.requests.filter((r) => r.path === '/v1/friend-requests');
      expect(sent).toHaveLength(1);
      expect(sent[0]?.body).toEqual({ search_ref: expect.stringMatching(/^ref/) });
      expect(sent[0]?.idempotencyKey).toMatch(/^[0-9a-f-]{36}$/);
      expect(server.requests.filter((r) => r.from === ME)).toHaveLength(1);
    },
  );

  it.each([['nobody'], ['hidden'], ['blocked'], ['me.cat']])(
    'shows the one "No one found" for %s',
    async (handle) => {
      const { user } = await render();
      await user.type(field(), handle);
      await user.press(searchButton());
      const none = await screen.findByTestId('find-none');
      expect(none.props.children).toBe(t('find.none'));
      expect(screen.queryByTestId('find-result')).toBeNull();
    },
  );

  it('rejects a malformed handle without a request', async () => {
    const { server, user } = await render();
    await user.type(field(), 'ma');
    await user.press(searchButton());
    expect(screen.getByText(t('find.field.format'))).toBeTruthy();
    expect(server.count('POST', '/v1/users/search')).toBe(0);
  });

  it('says try later when searches are rate limited', async () => {
    const { user } = await render('light', (s) =>
      s.overrides.set('POST /v1/users/search', problem(429, 'RATE_LIMITED')),
    );
    await user.type(field(), 'mai.cat');
    await user.press(searchButton());
    expect(await screen.findByText(t('find.error.tryLater'))).toBeTruthy();
  });

  it('follows the caps and reuses the key on retry', async () => {
    const { server, user } = await render('light', (s) =>
      s.overrides.set('POST /v1/friend-requests', problem(409, 'FRIEND_LIMIT_REACHED')),
    );
    await user.type(field(), 'mai.cat');
    await user.press(searchButton());
    await screen.findByTestId('find-result');
    const send = () =>
      user.press(screen.getByRole('button', { name: t('find.send.a11y', { name: 'Mai' }) }));
    await send();
    expect(await screen.findByText(t('friends.error.friendLimit'))).toBeTruthy();
    await send();
    await screen.findByTestId('find-sent');
    const keys = server.auth.requests
      .filter((r) => r.path === '/v1/friend-requests')
      .map((r) => r.idempotencyKey);
    expect(keys).toHaveLength(2);
    expect(keys[0]).toBe(keys[1]);
  });

  it('asks to search again when the ref expired', async () => {
    const { user } = await render('light', (s) =>
      s.overrides.set('POST /v1/friend-requests', problem(404, 'NOT_FOUND')),
    );
    await user.type(field(), 'mai.cat');
    await user.press(searchButton());
    await screen.findByTestId('find-result');
    await user.press(screen.getByRole('button', { name: t('find.send.a11y', { name: 'Mai' }) }));
    await waitFor(() => expect(screen.getByText(t('find.error.expired'))).toBeTruthy());
  });

  it('normalizes the typed handle', () => {
    expect(searchableHandle('  @Mai.Cat ')).toBe('mai.cat');
    expect(searchableHandle('@@x')).toBe('@x');
  });
});
