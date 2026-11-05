import { fireEvent, screen, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { problem } from '../../../test/fakeAuthServer';
import type { FakeSocialServer } from '../../../test/fakeSocialServer';
import { SCHEMES } from '../../../test/render';
import { renderSocial } from '../../../test/socialHarness';
import { PrivacyScreen } from '../PrivacyScreen';

const render = (prepare?: (s: FakeSocialServer) => void, scheme: 'light' | 'dark' = 'light') =>
  renderSocial((api) => <PrivacyScreen api={api} onBack={() => undefined} />, { prepare, scheme });

describe('PrivacyScreen', () => {
  it.each(SCHEMES)('turns findable off and on (%s)', async (scheme) => {
    const { server } = await render(undefined, scheme);
    const toggle = await screen.findByTestId('findable-switch');
    expect(toggle.props.value).toBe(true);
    fireEvent(toggle, 'valueChange', false);
    await waitFor(() => expect(screen.getByTestId('findable-switch').props.value).toBe(false));
    expect(server.findable).toBe(false);
    fireEvent(screen.getByTestId('findable-switch'), 'valueChange', true);
    await waitFor(() => expect(server.findable).toBe(true));
  });

  it('offers no switch when the account cannot be findable, without saying why', async () => {
    await render((s) => {
      s.canBeFindable = false;
    });
    expect(await screen.findByTestId('findable-unavailable')).toBeTruthy();
    expect(screen.queryByTestId('findable-switch')).toBeNull();
    expect(screen.queryByText(/age|old|minor|18/i)).toBeNull();
  });

  it('re-reads the settings when the server refuses findable', async () => {
    const { server } = await render((s) => {
      s.findable = false;
    });
    const toggle = await screen.findByTestId('findable-switch');
    server.canBeFindable = false; // e.g. region changed since the screen loaded
    fireEvent(toggle, 'valueChange', true);
    expect(await screen.findByTestId('findable-unavailable')).toBeTruthy();
    expect(server.findable).toBe(false);
  });

  it('lists blocked accounts and unblocks without restoring the friendship', async () => {
    let id = '';
    const { server, user } = await render((s) => {
      const mai = s.addUser('Mai', { handle: 'mai.cat' });
      id = mai.id;
      s.blocks.set(mai.id, s.now());
    });
    await screen.findByTestId(`blocked-${id}`);
    await user.press(
      screen.getByRole('button', { name: t('privacy.unblock.a11y', { name: 'Mai' }) }),
    );
    await waitFor(() => expect(screen.queryByTestId(`blocked-${id}`)).toBeNull());
    expect(server.blocks.has(id)).toBe(false);
    expect(server.friends.has(id)).toBe(false);
    expect(screen.getByText(t('privacy.blocked.empty'))).toBeTruthy();
  });

  it('offers a retry when settings cannot load', async () => {
    const { user } = await render((s) =>
      s.overrides.set('GET /v1/me', problem(503, 'UNAVAILABLE')),
    );
    await screen.findByText(t('privacy.error.load'));
    await user.press(screen.getAllByRole('button', { name: t('friends.retry') })[0]!);
    expect(await screen.findByTestId('findable-switch')).toBeTruthy();
  });
});
