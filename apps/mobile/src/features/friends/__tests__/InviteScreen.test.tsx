import { screen, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { problem } from '../../../test/fakeAuthServer';
import { ME, SHARE_HOST, tokenFor, type FakeSocialServer } from '../../../test/fakeSocialServer';
import { SCHEMES } from '../../../test/render';
import { renderSocial } from '../../../test/socialHarness';
import { InvitePreviewScreen } from '../InvitePreviewScreen';
import { InviteScreen } from '../InviteScreen';

// Skia draws natively; the QR geometry is covered by src/qr/__tests__/qrEncoder.test.ts.
jest.mock('@shopify/react-native-skia', () => {
  const { View } = jest.requireActual('react-native');
  return { Canvas: View, Path: () => null };
});

const DAY = 24 * 3_600_000;

describe('InviteScreen', () => {
  async function setup(
    scheme: 'light' | 'dark' = 'light',
    prepare?: (s: FakeSocialServer) => void,
  ) {
    const shared: string[] = [];
    const rendered = await renderSocial(
      (api) => (
        <InviteScreen
          api={api}
          onBack={() => undefined}
          share={async (message) => {
            shared.push(message);
          }}
          now={() => Date.parse('2026-11-05T10:00:00Z')}
        />
      ),
      { scheme, prepare },
    );
    return { ...rendered, shared };
  }

  it.each(SCHEMES)('shows the link as text and QR code, and shares it (%s)', async (scheme) => {
    const { user, shared } = await setup(scheme);
    const url = `${SHARE_HOST}/i/${tokenFor('mine')}`;
    expect(await screen.findByText(url)).toBeTruthy();
    expect(screen.getByLabelText(t('invite.qr.a11y'))).toBeTruthy();
    expect(screen.queryByText(/This link works for/)).toBeNull();
    await user.press(screen.getByRole('button', { name: t('invite.share') }));
    expect(shared).toEqual([t('invite.share.message', { url })]);
  });

  it('shows how long a minor invite still works', async () => {
    await setup('light', (s) => {
      s.myInviteExpiresAt = new Date(Date.parse('2026-11-05T10:00:00Z') + 6.5 * DAY).toISOString();
    });
    expect(await screen.findByText(t('invite.expires.days', { count: 6 }))).toBeTruthy();
  });

  it('resets the link only after the confirm', async () => {
    const { user, server } = await setup();
    const oldUrl = `${SHARE_HOST}/i/${tokenFor('mine')}`;
    await screen.findByText(oldUrl);
    await user.press(screen.getByRole('button', { name: t('invite.reset') }));
    expect(server.count('POST', '/v1/me/invite/reset')).toBe(0);
    const confirm = screen.getAllByRole('button', { name: t('invite.reset') }).at(-1);
    if (!confirm) throw new Error('no confirm');
    await user.press(confirm);
    await waitFor(() => expect(screen.queryByText(oldUrl)).toBeNull());
    expect(screen.getByTestId('invite-url').props.children).toBe(
      `${SHARE_HOST}/i/${server.myInviteToken}`,
    );
    expect(server.expiredInvites.has(tokenFor('mine'))).toBe(true);
  });

  it('offers a retry when the link cannot load', async () => {
    const { user } = await setup('light', (s) =>
      s.overrides.set('GET /v1/me/invite', problem(503, 'UNAVAILABLE')),
    );
    await screen.findByText(t('invite.error.load'));
    await user.press(screen.getByRole('button', { name: t('friends.retry') }));
    expect(await screen.findByTestId('invite-url')).toBeTruthy();
  });
});

describe('InvitePreviewScreen', () => {
  const render = (token: string, prepare?: (s: FakeSocialServer) => void, scheme?: 'dark') =>
    renderSocial(
      (api) => <InvitePreviewScreen api={api} token={token} onDone={() => undefined} />,
      {
        prepare,
        scheme,
      },
    );

  it.each(SCHEMES)('sends one request for an opened invite (%s)', async (scheme) => {
    let maiId = '';
    const { server, user } = await render(
      tokenFor('mai'),
      (s) => {
        const mai = s.addUser('Mai');
        maiId = mai.id;
        s.invites.set(tokenFor('mai'), mai.id);
      },
      scheme === 'dark' ? 'dark' : undefined,
    );
    expect(await screen.findByText(t('invite.preview.title', { name: 'Mai' }))).toBeTruthy();
    await user.press(screen.getByRole('button', { name: t('invite.preview.add') }));
    expect(await screen.findByTestId('invite-sent')).toBeTruthy();
    const sent = server.auth.requests.filter((r) => r.path === '/v1/friend-requests');
    expect(sent).toHaveLength(1);
    expect(sent[0]?.idempotencyKey).toMatch(/^[0-9a-f-]{36}$/);
    expect(server.requests.some((r) => r.from === ME && r.to === maiId)).toBe(true);
  });

  it('keeps the same idempotency key when a send is retried', async () => {
    const { server, user } = await render(tokenFor('mai'), (s) => {
      s.invites.set(tokenFor('mai'), s.addUser('Mai').id);
      s.overrides.set('POST /v1/friend-requests', problem(503, 'UNAVAILABLE'));
    });
    await screen.findByText(t('invite.preview.title', { name: 'Mai' }));
    await user.press(screen.getByRole('button', { name: t('invite.preview.add') }));
    expect(await screen.findByText(t('friends.error.generic'))).toBeTruthy();
    await user.press(screen.getByRole('button', { name: t('invite.preview.add') }));
    await screen.findByTestId('invite-sent');
    const keys = server.auth.requests
      .filter((r) => r.path === '/v1/friend-requests')
      .map((r) => r.idempotencyKey);
    expect(keys).toHaveLength(2);
    expect(keys[0]).toBe(keys[1]);
  });

  it('says "That’s you!" for my own invite and offers nothing to send', async () => {
    await render(tokenFor('mine'));
    expect(await screen.findByTestId('invite-self')).toBeTruthy();
    expect(screen.queryByRole('button', { name: t('invite.preview.add') })).toBeNull();
  });

  it.each([
    ['unknown', tokenFor('nobody'), undefined],
    ['reset', tokenFor('old'), (s: FakeSocialServer) => s.expiredInvites.add(tokenFor('old'))],
    ['malformed', '../../me', undefined],
  ] as const)('reads the same for an %s invite', async (_, token, prepare) => {
    const { server } = await render(token, prepare);
    const node = await screen.findByTestId('invite-unusable');
    expect(node.props.children).toBe(t('invite.preview.unusable'));
    if (token === '../../me') expect(server.count('GET', '/v1/invites')).toBe(0);
  });

  it('shows the friend-list-full message and keeps the button', async () => {
    const { user } = await render(tokenFor('mai'), (s) => {
      s.invites.set(tokenFor('mai'), s.addUser('Mai').id);
      for (let i = 0; i < 20; i += 1) s.friends.add(s.addUser(`F${i}`).id);
    });
    await screen.findByText(t('invite.preview.title', { name: 'Mai' }));
    await user.press(screen.getByRole('button', { name: t('invite.preview.add') }));
    expect(await screen.findByText(t('friends.error.friendLimit'))).toBeTruthy();
    expect(screen.getByRole('button', { name: t('invite.preview.add') })).toBeTruthy();
  });
});
