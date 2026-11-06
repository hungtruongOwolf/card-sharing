import { screen, userEvent, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { renderThemed, SCHEMES } from '../../../test/render';
import { memoryKeyValueStore } from '../../drafts/draftStore';
import type { CardsApi } from '../../sharing/cardsApi';
import { createOutbox } from '../outbox';
import { createOutboxStore, type OutboxEntry } from '../outboxStore';
import { OutboxScreen } from '../OutboxScreen';

const CARD = 'c0000000-0000-4000-8000-000000000001';
const JOB_ID = '0c0c0c0c-0c0c-4c0c-8c0c-0c0c0c0c0c0c';

const entries: OutboxEntry[] = [
  {
    kind: 'send',
    id: JOB_ID,
    job: {
      clientCardId: JOB_ID,
      cardId: JOB_ID,
      audience: { mode: 'all' },
      keepOriginal: false,
      style: { cut: { id: 'cut.none', version: 1, params: {} } },
      caption: 'beach day',
      media: null,
    },
    createdAt: 1,
    attempts: 3,
    nextAttemptAt: 2,
    state: 'failed',
  },
  {
    kind: 'reaction',
    id: `r-${CARD}`,
    cardId: CARD,
    reaction: 'react.heart',
    createdAt: 2,
    attempts: 0,
    nextAttemptAt: 3,
    state: 'pending',
  },
];

async function setup(scheme: 'light' | 'dark' = 'light') {
  const kv = memoryKeyValueStore();
  const store = createOutboxStore(kv);
  for (const entry of entries) await store.put(entry);
  const outbox = createOutbox({
    store,
    api: {} as CardsApi,
    prepareSend: () => Promise.reject(new Error('offline')),
    removeDraft: () => Promise.resolve(),
    now: () => 0,
  });
  jest.spyOn(outbox, 'retry').mockResolvedValue();
  await renderThemed(<OutboxScreen outbox={outbox} entries={entries} onBack={() => undefined} />, {
    scheme,
  });
  return { outbox, kv };
}

describe('OutboxScreen', () => {
  it.each(SCHEMES)('shows pending and not-sent items (%s)', async (scheme) => {
    await setup(scheme);
    expect(screen.getByText('beach day')).toBeTruthy();
    expect(screen.getByText(t('outbox.state.failed'))).toBeTruthy();
    expect(screen.getByText(t('outbox.reaction', { glyph: '❤️' }))).toBeTruthy();
    expect(screen.getByText(t('outbox.state.pending'))).toBeTruthy();
  });

  it('retries or deletes a not-sent card and cancels a pending reaction', async () => {
    const { outbox, kv } = await setup();
    const user = userEvent.setup();
    await user.press(
      screen.getByRole('button', { name: t('outbox.retry.a11y', { name: 'beach day' }) }),
    );
    expect(outbox.retry).toHaveBeenCalledWith(JOB_ID);
    await user.press(
      screen.getByRole('button', { name: t('outbox.delete.a11y', { name: 'beach day' }) }),
    );
    const reaction = t('outbox.reaction', { glyph: '❤️' });
    await user.press(
      screen.getByRole('button', { name: t('outbox.cancel.a11y', { name: reaction }) }),
    );
    await waitFor(() => expect(kv.items.get('cs.outbox.index.v1')).toBe('[]'));
  });
});
