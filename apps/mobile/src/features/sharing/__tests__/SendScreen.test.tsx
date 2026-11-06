import { fireEvent, screen, waitFor } from '@testing-library/react-native';

import { registry } from '@card-sharing/effects-registry';

import { t } from '../../../i18n';
import { renderCards } from '../../../test/cardsHarness';
import { problem } from '../../../test/fakeAuthServer';
import { fakeJpeg, fakePng, type FakeCardsServer } from '../../../test/fakeCardsServer';
import { SCHEMES } from '../../../test/render';
import { newLocalCard, type LocalCard } from '../../cards/localCard';
import type { PreparedCard } from '../../cards/prepare';
import { styleChoice } from '../../editor/editorState';
import type { SendJob } from '../sendCard';
import { SendScreen } from '../SendScreen';

const CARD_ID = '0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c';

function localCard(cut = 'cut.none'): LocalCard {
  return newLocalCard(
    { uri: 'file:///cache/a.jpg', width: 3, height: 4, origin: 'camera' },
    {
      cut: styleChoice(registry.get(cut)!),
      finish: null,
      effects: [],
      wholePhoto: false,
      caption: ' zoomies ',
    },
    new Date('2026-11-05T09:00:00Z'),
    CARD_ID,
  );
}

const prepared = (withMask: boolean, keepOriginal: boolean): PreparedCard => ({
  render: { width: 72, height: 96, contentType: 'image/png', bytes: fakePng(72, 96) },
  mask: withMask
    ? { width: 72, height: 96, contentType: 'image/png', bytes: fakePng(72, 96, 1) }
    : null,
  original: keepOriginal
    ? { width: 300, height: 400, contentType: 'image/jpeg', bytes: fakeJpeg() }
    : null,
});

function seed(server: FakeCardsServer) {
  const mai = server.addUser('Mai');
  const ben = server.addUser('Ben');
  server.friends.add(mai.id);
  server.friends.add(ben.id);
  return { mai, ben };
}

async function setup({
  scheme = 'light' as const,
  card = localCard(),
  withMask = false,
  queue,
  extra,
}: {
  scheme?: 'light' | 'dark';
  card?: LocalCard;
  withMask?: boolean;
  queue?: (job: SendJob) => Promise<boolean>;
  extra?: (server: FakeCardsServer) => void;
} = {}) {
  let people!: ReturnType<typeof seed>;
  const prepare = jest.fn((_card: LocalCard, keep: boolean) =>
    Promise.resolve(prepared(withMask, keep)),
  );
  const removeDraft = jest.fn(() => Promise.resolve());
  const onDone = jest.fn();
  const rendered = await renderCards(
    (apis) => (
      <SendScreen
        card={card}
        cardsApi={apis.cards}
        friendsApi={apis.friends}
        prepare={prepare}
        removeDraft={removeDraft}
        statusPollMs={10}
        queue={queue}
        onEdit={() => undefined}
        onOpenFriends={() => undefined}
        onDone={onDone}
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
  return { ...rendered, ...people, prepare, removeDraft, onDone };
}

const sendButton = () => screen.getByRole('button', { name: t('send.send') });

describe('SendScreen', () => {
  it.each(SCHEMES)('defaults to all friends with keep-original off (%s)', async (scheme) => {
    const { server, prepare } = await setup({ scheme });
    const all = await screen.findByRole('radio', { name: t('send.audience.all', { count: 2 }) });
    expect(all.props.accessibilityState).toMatchObject({ checked: true });
    expect(screen.getByLabelText(t('send.keepOriginal')).props.value).toBe(false);
    // Nothing is baked or uploaded before the tap.
    expect(prepare).not.toHaveBeenCalled();
    expect(server.count('POST', '/v1/media')).toBe(0);
    expect(server.count('POST', '/v1/cards')).toBe(0);
  });

  it('sends the render only (no mask, no original) to all friends, then drops the draft', async () => {
    const { server, user, removeDraft } = await setup();
    await screen.findByRole('radio', { name: t('send.audience.all', { count: 2 }) });
    await user.press(sendButton());
    expect(await screen.findByTestId('send-status-pending')).toBeTruthy();
    expect(screen.getByText(t('send.status.pending', { count: 2 }))).toBeTruthy();
    expect([...server.media.values()].map((m) => m.kind)).toEqual(['card_render']);
    const [card] = server.sentCards();
    expect(card).toMatchObject({
      clientCardId: CARD_ID,
      maskMediaId: null,
      originalMediaId: null,
      caption: 'zoomies',
    });
    expect(card!.recipients).toHaveLength(2);
    const create = server.auth.requests.find((r) => r.path === '/v1/cards');
    expect(create?.idempotencyKey).toBe(CARD_ID);
    expect(create?.body).toMatchObject({
      audience: { mode: 'all' },
      style: { cut: { id: 'cut.none', version: 1 } },
    });
    expect(removeDraft).toHaveBeenCalledWith(CARD_ID);
  });

  it('uploads the mask when the cut style uses it and the original only with the toggle on', async () => {
    const { server, user, prepare } = await setup({
      card: localCard('cut.diecut'),
      withMask: true,
    });
    await screen.findByRole('radio', { name: t('send.audience.all', { count: 2 }) });
    await fireEvent(screen.getByLabelText(t('send.keepOriginal')), 'valueChange', true);
    await user.press(sendButton());
    await screen.findByTestId('send-status-pending');
    expect(prepare).toHaveBeenCalledWith(expect.anything(), true);
    expect([...server.media.values()].map((m) => m.kind)).toEqual([
      'card_render',
      'mask',
      'original',
    ]);
    const [card] = server.sentCards();
    expect(card!.maskMediaId).not.toBeNull();
    expect(card!.originalMediaId).not.toBeNull();
  });

  it('sends to the chosen friends only', async () => {
    const { server, user, mai } = await setup();
    await user.press(await screen.findByRole('radio', { name: t('send.audience.some') }));
    expect(sendButton().props.accessibilityState).toMatchObject({ disabled: true });
    await user.press(
      screen.getByRole('checkbox', { name: t('send.friend.a11y', { name: 'Mai' }) }),
    );
    expect(screen.getByText(t('send.audience.picked', { count: 1 }))).toBeTruthy();
    await user.press(sendButton());
    await screen.findByTestId('send-status-pending');
    expect(server.sentCards()[0]!.recipients).toEqual([mai.id]);
    expect(server.auth.requests.find((r) => r.path === '/v1/cards')?.body).toMatchObject({
      audience: { mode: 'subset', recipient_ids: [mai.id] },
    });
  });

  it('retries a lost answer with the same key and creates exactly one card', async () => {
    const { server, user } = await setup({
      extra: (s) => s.loseResponse.add('POST /v1/cards'),
    });
    await screen.findByRole('radio', { name: t('send.audience.all', { count: 2 }) });
    await user.press(sendButton());
    expect(await screen.findByText(t('send.error.offline'))).toBeTruthy();
    // The choices are locked: the retry must be the same request.
    expect(screen.getByLabelText(t('send.keepOriginal')).props.disabled).toBe(true);
    await user.press(screen.getByRole('button', { name: t('send.retry') }));
    await screen.findByTestId('send-status-pending');
    const creates = server.auth.requests.filter((r) => r.path === '/v1/cards');
    expect(creates).toHaveLength(2);
    expect(new Set(creates.map((r) => r.idempotencyKey))).toEqual(new Set([CARD_ID]));
    expect(server.sentCards()).toHaveLength(1);
    // Uploads finished before the loss; the retry reuses their ids instead of uploading again.
    expect(server.count('POST', '/v1/media')).toBe(1);
  });

  it('shows the held state neutrally with the review entry point', async () => {
    const { server, user } = await setup();
    await screen.findByRole('radio', { name: t('send.audience.all', { count: 2 }) });
    await user.press(sendButton());
    await screen.findByTestId('send-status-pending');
    server.sentCards()[0]!.status = 'held_review';
    expect(await screen.findByTestId('send-status-held')).toBeTruthy();
    expect(screen.getByText(t('send.status.held.body'))).toBeTruthy();
    expect(
      screen.getByRole('button', { name: t('send.appeal') }).props.accessibilityState,
    ).toMatchObject({ disabled: true });
  });

  it('shows delivery once the checks pass', async () => {
    const { server, user, onDone } = await setup();
    await screen.findByRole('radio', { name: t('send.audience.all', { count: 2 }) });
    await user.press(sendButton());
    await screen.findByTestId('send-status-pending');
    server.sentCards()[0]!.status = 'active';
    expect(await screen.findByText(t('send.status.active', { count: 2 }))).toBeTruthy();
    await user.press(screen.getByRole('button', { name: t('send.done') }));
    expect(onDone).toHaveBeenCalled();
  });

  it('refuses a card whose style no longer exists, before any upload', async () => {
    const stale = localCard();
    const card = {
      ...stale,
      draft: { ...stale.draft, cut: { id: 'cut.none', version: 99, params: {} } },
    };
    const { server, user, prepare } = await setup({ card });
    await screen.findByRole('radio', { name: t('send.audience.all', { count: 2 }) });
    await user.press(sendButton());
    expect(await screen.findByText(t('send.error.staleStyle'))).toBeTruthy();
    expect(prepare).not.toHaveBeenCalled();
    expect(server.count('POST', '/v1/media')).toBe(0);
  });

  it('explains the daily limit and keeps the same job for a later retry', async () => {
    const { user } = await setup({
      extra: (s) => s.overrides.set('POST /v1/cards', problem(429, 'RATE_LIMITED')),
    });
    await screen.findByRole('radio', { name: t('send.audience.all', { count: 2 }) });
    await user.press(sendButton());
    expect(await screen.findByText(t('send.error.rateLimited'))).toBeTruthy();
    expect(screen.getByRole('button', { name: t('send.retry') })).toBeTruthy();
  });

  it('asks to add friends when there are none', async () => {
    await renderCards((apis) => (
      <SendScreen
        card={localCard()}
        cardsApi={apis.cards}
        friendsApi={apis.friends}
        prepare={() => Promise.resolve(prepared(false, false))}
        removeDraft={() => Promise.resolve()}
        onEdit={() => undefined}
        onOpenFriends={() => undefined}
        onDone={() => undefined}
      />
    ));
    expect(await screen.findByTestId('send-no-friends')).toBeTruthy();
    await waitFor(() =>
      expect(sendButton().props.accessibilityState).toMatchObject({ disabled: true }),
    );
  });

  it('hands a send that cannot go out now to the outbox with its job', async () => {
    const queue = jest.fn((_job: SendJob) => Promise.resolve(true));
    const { server, user, removeDraft } = await setup({
      queue,
      extra: (s) => s.overrides.set('POST /v1/cards', problem(503, 'TEMPORARILY_UNAVAILABLE')),
    });
    await screen.findByRole('radio', { name: t('send.audience.all', { count: 2 }) });
    await user.press(sendButton());
    expect(await screen.findByTestId('send-queued')).toBeTruthy();
    expect(queue).toHaveBeenCalledWith(
      expect.objectContaining({ clientCardId: CARD_ID, media: expect.anything() }),
    );
    expect(server.sentCards()).toHaveLength(0);
    // The draft stays until the outbox delivers it.
    expect(removeDraft).not.toHaveBeenCalled();
  });

  it('does not queue a send the server refused', async () => {
    const queue = jest.fn(() => Promise.resolve(true));
    const { user } = await setup({
      queue,
      extra: (s) => s.overrides.set('POST /v1/cards', problem(403, 'ACCOUNT_SUSPENDED')),
    });
    await screen.findByRole('radio', { name: t('send.audience.all', { count: 2 }) });
    await user.press(sendButton());
    expect(await screen.findByText(t('send.error.notAllowed'))).toBeTruthy();
    expect(queue).not.toHaveBeenCalled();
  });
});
