/**
 * Public board links in the app (T-134): the API client against the T-129 contract and the manage sheet.
 */
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, screen, userEvent, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import { memoryPublicLinkPrefs } from '../publicLinkPrefs';
import {
  createPublicLinksApi,
  LinksDisallowedError,
  parseCreated,
  type CreatedLink,
  type PublicLink,
  type PublicLinksApi,
} from '../publicLinksApi';
import { PublicLinksSheet } from '../PublicLinksSheet';

const BOARD = '0192f0cc-0000-7000-8000-000000000001';
const LINK = '0192f0cc-0000-7000-8000-0000000000a1';
const URL_ONCE = 'https://share.example/b/3q2-7wAAAAAAAAAAAAAAAA';

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': status >= 400 ? 'application/problem+json' : 'application/json' },
  });

describe('public links api (T-129 contract)', () => {
  it('creates with an expiry and an idempotency key, lists without URLs and revokes', async () => {
    const calls: { path: string; init: RequestInit }[] = [];
    const api = createPublicLinksApi(async (path, init = {}) => {
      calls.push({ path, init });
      if (init.method === 'POST') {
        return json(201, {
          link: {
            id: LINK,
            board_id: BOARD,
            created_at: '2026-11-06T00:00:00Z',
            expires_at: null,
            url: URL_ONCE,
          },
        });
      }
      if (init.method === 'DELETE') return new Response(null, { status: 204 });
      return json(200, {
        items: [
          {
            id: LINK,
            board_id: BOARD,
            created_at: '2026-11-06T00:00:00Z',
            expires_at: '2026-11-13T00:00:00Z',
          },
        ],
      });
    });
    const created = await api.create(BOARD, 'never', 'key-1');
    expect(created).toMatchObject({ id: LINK, url: URL_ONCE, expiresAt: null });
    expect(calls[0]).toMatchObject({
      path: `/v1/boards/${BOARD}/public-links`,
      init: {
        headers: { 'Idempotency-Key': 'key-1' },
        body: JSON.stringify({ expires_in: 'never' }),
      },
    });
    expect(await api.list(BOARD)).toEqual([
      {
        id: LINK,
        createdAt: Date.parse('2026-11-06T00:00:00Z'),
        expiresAt: Date.parse('2026-11-13T00:00:00Z'),
      },
    ]);
    await api.revoke(LINK);
    expect(calls.at(-1)).toMatchObject({
      path: `/v1/public-links/${LINK}`,
      init: { method: 'DELETE' },
    });
  });

  it('turns a policy refusal into LinksDisallowedError (AC-24.6)', async () => {
    const api = createPublicLinksApi(async () =>
      json(403, { type: 'about:blank', status: 403, code: 'POLICY_DISALLOWED' }),
    );
    await expect(api.create(BOARD, '7d', 'k')).rejects.toBeInstanceOf(LinksDisallowedError);
  });

  it('accepts only an https share-page URL from the server', () => {
    const body = (url: string) => ({
      link: {
        id: LINK,
        board_id: BOARD,
        created_at: '2026-11-06T00:00:00Z',
        expires_at: null,
        url,
      },
    });
    expect(parseCreated(body(URL_ONCE))?.url).toBe(URL_ONCE);
    for (const bad of [
      'javascript:alert(1)',
      'http://share.example/b/3q2-7wAAAAAAAAAAAAAAAA',
      'https://x/y',
    ]) {
      expect(parseCreated(body(bad))).toBeNull();
    }
  });
});

function fakeApi(over: Partial<PublicLinksApi> = {}) {
  let links: PublicLink[] = [];
  const api: PublicLinksApi = {
    list: jest.fn(async () => links),
    create: jest.fn(async (): Promise<CreatedLink> => {
      const link = {
        id: LINK,
        createdAt: 0,
        expiresAt: Date.parse('2026-11-13T00:00:00Z'),
        url: URL_ONCE,
      };
      links = [link];
      return link;
    }),
    revoke: jest.fn(async () => {
      links = [];
    }),
    ...over,
  };
  return api;
}

async function open(api: PublicLinksApi, prefs = memoryPublicLinkPrefs()) {
  const onDisallowed = jest.fn();
  const onClose = jest.fn();
  const share = jest.fn(async () => undefined);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  await renderThemed(
    <QueryClientProvider client={client}>
      <PublicLinksSheet
        visible
        boardId={BOARD}
        api={api}
        prefs={prefs}
        onClose={onClose}
        onDisallowed={onDisallowed}
        share={share}
      />
    </QueryClientProvider>,
  );
  return { onDisallowed, onClose, share, prefs };
}

describe('PublicLinksSheet (T-134)', () => {
  it('explains link previews once, then creates, shares and revokes a link', async () => {
    const api = fakeApi();
    const { share, prefs } = await open(api);
    const user = userEvent.setup();
    expect(await screen.findByText(t('boards.links.disclosure'))).toBeTruthy();
    expect(api.list).not.toHaveBeenCalled();
    await user.press(screen.getByRole('button', { name: t('boards.links.disclosure.confirm') }));
    expect(await prefs.disclosureSeen()).toBe(true);

    await user.press(screen.getByRole('radio', { name: t('boards.links.expiry.24h') }));
    await user.press(screen.getByRole('button', { name: t('boards.links.create') }));
    expect(api.create).toHaveBeenCalledWith(BOARD, '24h', expect.any(String));
    expect(await screen.findByText(URL_ONCE)).toBeTruthy();
    await user.press(screen.getByRole('button', { name: t('boards.links.share') }));
    expect(share).toHaveBeenCalledWith(URL_ONCE);

    await act(async () => {
      await user.press(await screen.findByText(t('boards.links.revoke')));
    });
    expect(api.revoke).toHaveBeenCalledWith(LINK);
    await waitFor(() => expect(screen.queryByText(URL_ONCE)).toBeNull());
  });

  it('skips the explanation once seen', async () => {
    await open(fakeApi(), memoryPublicLinkPrefs({ disclosureSeen: true }));
    expect(await screen.findByText(t('boards.links.none'))).toBeTruthy();
    expect(screen.queryByText(t('boards.links.disclosure'))).toBeNull();
  });

  it('remembers a policy refusal and tells the board to hide the action (AC-24.6)', async () => {
    const api = fakeApi({
      create: jest.fn(async () => {
        throw new LinksDisallowedError('no');
      }),
    });
    const { onDisallowed, onClose, prefs } = await open(
      api,
      memoryPublicLinkPrefs({ disclosureSeen: true }),
    );
    await userEvent
      .setup()
      .press(await screen.findByRole('button', { name: t('boards.links.create') }));
    await waitFor(() => expect(onDisallowed).toHaveBeenCalled());
    expect(onClose).toHaveBeenCalled();
    expect(await prefs.disallowed()).toBe(true);
  });
});
