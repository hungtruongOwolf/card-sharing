import { parseEntry } from '../../outbox/outboxStore';
import { createCardBody, parseCard, type NewCard } from '../cardsApi';
import { parseReplyTarget, replyParams } from '../replyTarget';

const CARD = 'c0000000-0000-4000-8000-000000000001';
const SENDER = 'a0000000-0000-4000-8000-000000000002';

const newCard = (extra: Partial<NewCard> = {}): NewCard => ({
  clientCardId: 'd0000000-0000-4000-8000-000000000003',
  renderMediaId: 'e0000000-0000-4000-8000-000000000004',
  maskMediaId: null,
  originalMediaId: null,
  style: { cut: { id: 'cut.none', version: 1, params: {} } } as unknown as NewCard['style'],
  caption: null,
  audience: { mode: 'subset', recipientIds: [SENDER] },
  ...extra,
});

describe('reply target route params (ADR-0010)', () => {
  it('round-trips ids and refuses anything else', () => {
    const target = { cardId: CARD, senderId: SENDER };
    expect(parseReplyTarget(replyParams(target))).toEqual(target);
    expect(replyParams(null)).toEqual({});
    expect(parseReplyTarget({})).toBeNull();
    expect(parseReplyTarget({ replyTo: 'x', replyToSender: SENDER })).toBeNull();
    expect(parseReplyTarget({ replyTo: [CARD], replyToSender: SENDER })).toBeNull();
  });
});

describe('reply cards on the wire', () => {
  it('adds reply_to_card_id only on a reply, so other bodies keep their exact bytes', () => {
    expect(createCardBody(newCard())).not.toHaveProperty('reply_to_card_id');
    expect(createCardBody(newCard({ replyToCardId: CARD }))).toMatchObject({
      reply_to_card_id: CARD,
      audience: { mode: 'subset', recipient_ids: [SENDER] },
    });
    expect(() => createCardBody(newCard({ replyToCardId: 'nope' }))).toThrow();
  });

  it('reads reply_to from a card view, ignoring malformed ids', () => {
    const view = {
      id: CARD,
      sender: { id: SENDER, display_name: 'Mai', avatar_url: null },
      caption: null,
      alt_text: null,
      style: {},
      media: null,
      sent_at: '2026-11-07T10:00:00Z',
      is_mine: false,
      my_reaction: null,
    };
    expect(parseCard({ ...view, reply_to: { card_id: CARD } })?.replyTo).toEqual({ cardId: CARD });
    expect(parseCard({ ...view, reply_to: null })).not.toHaveProperty('replyTo');
    expect(parseCard({ ...view, reply_to: { card_id: 'x' } })).not.toHaveProperty('replyTo');
  });

  it('keeps a queued reply a reply in the outbox, and drops a tampered target', () => {
    const job = {
      clientCardId: 'd0000000-0000-4000-8000-000000000003',
      cardId: 'local-1',
      audience: { mode: 'subset', recipientIds: [SENDER] },
      keepOriginal: false,
      style: { cut: { id: 'cut.none', version: 1, params: {} } },
      caption: null,
      media: null,
    };
    const entry = (extra: Record<string, unknown>) =>
      JSON.stringify({
        id: job.clientCardId,
        kind: 'send',
        job: { ...job, ...extra },
        createdAt: 1,
        nextAttemptAt: 1,
        attempts: 0,
        state: 'pending',
      });
    expect(parseEntry(entry({}))).not.toBeNull();
    const parsed = parseEntry(entry({ replyToCardId: CARD }));
    expect(parsed?.kind === 'send' && parsed.job.replyToCardId).toBe(CARD);
    expect(parseEntry(entry({ replyToCardId: 'x' }))).toBeNull();
  });
});
