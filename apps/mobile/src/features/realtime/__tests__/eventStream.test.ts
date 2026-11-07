import { ACTIVITY_QUERY_KEY } from '../../activity/activityQuery';
import { cardQueryKey, FEED_QUERY_KEY } from '../../feed/queryKeys';
import {
  backoffMs,
  cardRepliesQueryKey,
  invalidationsFor,
  runEventStream,
  type FetchStream,
} from '../eventStream';
import { parseSse, type SseFrame } from '../sse';

const CARD = 'c0000000-0000-4000-8000-000000000001';
const ORIGINAL = 'c0000000-0000-4000-8000-000000000002';

const bytes = (text: string) => Uint8Array.from(text, (c) => c.charCodeAt(0));

/** A response whose body yields `chunks`, like `expo/fetch` and the browser fetch. */
function streamResponse(
  chunks: string[],
  init: { status?: number; headers?: Record<string, string> } = {},
) {
  const queue = chunks.map(bytes);
  const reader = {
    read: async () => {
      const value = queue.shift();
      return value ? { done: false, value } : { done: true };
    },
    cancel: jest.fn(async () => undefined),
  };
  const status = init.status ?? 200;
  return {
    response: {
      ok: status >= 200 && status < 300,
      status,
      headers: { get: (name: string) => init.headers?.[name] ?? null },
      body: { getReader: () => reader },
    } as unknown as Response,
    reader,
  };
}

describe('parseSse', () => {
  it('splits frames, skips comments and keeps the unfinished rest', () => {
    const { frames, rest } = parseSse(
      `retry: 3000\nevent: ready\ndata: {}\n\n: hb\n\nevent: card.delivered\ndata: {"card_id":"${CARD}"}\n\nevent: card.re`,
    );
    expect(frames).toEqual([
      { event: 'ready', data: {} },
      { event: 'card.delivered', data: { card_id: CARD } },
    ]);
    expect(rest).toBe('event: card.re');
  });

  it('treats non-object data as null (untrusted input)', () => {
    expect(parseSse('event: x\ndata: [1]\n\nevent: y\ndata: nope\n\n').frames).toEqual([
      { event: 'x', data: null },
      { event: 'y', data: null },
    ]);
  });

  it('accepts CRLF line endings', () => {
    expect(parseSse('event: ready\r\ndata: {}\r\n\r\n').frames).toEqual([
      { event: 'ready', data: {} },
    ]);
  });
});

describe('invalidationsFor', () => {
  const frame = (event: string, data: Record<string, unknown> | null = {}): SseFrame => ({
    event,
    data,
  });

  it('catches up on (re)connect and resync', () => {
    expect(invalidationsFor(frame('ready'))).toEqual([FEED_QUERY_KEY, ACTIVITY_QUERY_KEY]);
    expect(invalidationsFor(frame('resync'))).toEqual([FEED_QUERY_KEY, ACTIVITY_QUERY_KEY]);
  });

  it('refetches the feed for deliveries and replies, and the card for reactions', () => {
    expect(invalidationsFor(frame('card.delivered', { card_id: CARD }))).toEqual([
      FEED_QUERY_KEY,
      ACTIVITY_QUERY_KEY,
    ]);
    expect(invalidationsFor(frame('card.reply', { card_id: CARD, reply_to: ORIGINAL }))).toEqual([
      FEED_QUERY_KEY,
      ACTIVITY_QUERY_KEY,
      cardRepliesQueryKey(ORIGINAL),
    ]);
    expect(invalidationsFor(frame('card.reaction', { card_id: CARD }))).toEqual([
      ACTIVITY_QUERY_KEY,
      cardQueryKey(CARD),
    ]);
  });

  it('ignores malformed ids and unknown events', () => {
    expect(invalidationsFor(frame('card.reaction', { card_id: '../x' }))).toEqual([
      ACTIVITY_QUERY_KEY,
    ]);
    expect(invalidationsFor(frame('reconnect', { reason: 'max_age' }))).toEqual([]);
    expect(invalidationsFor(frame('something.else'))).toEqual([]);
  });
});

describe('runEventStream', () => {
  it('reads frames across chunk boundaries until the server asks to reconnect', async () => {
    const { response, reader } = streamResponse([
      'event: ready\ndata: {}\n\nevent: card.deli',
      `vered\ndata: {"card_id":"${CARD}"}\n\n`,
      'event: reconnect\ndata: {"reason":"max_age"}\n\n',
      'event: card.delivered\ndata: {}\n\n',
    ]);
    const fetchStream: FetchStream = jest.fn(async () => response);
    const seen: string[] = [];
    const end = await runEventStream(
      fetchStream,
      (f) => seen.push(f.event),
      new AbortController().signal,
    );
    expect(end).toEqual({ kind: 'closed' });
    expect(seen).toEqual(['ready', 'card.delivered', 'reconnect']);
    expect(fetchStream).toHaveBeenCalledWith(
      '/v1/events',
      expect.objectContaining({ headers: { Accept: 'text/event-stream' } }),
    );
    expect(reader.cancel).toHaveBeenCalled();
  });

  it('reports refusals with Retry-After and a missing body as unsupported', async () => {
    const busy = streamResponse([], { status: 503, headers: { 'Retry-After': '5' } }).response;
    await expect(
      runEventStream(async () => busy, jest.fn(), new AbortController().signal),
    ).resolves.toEqual({ kind: 'refused', status: 503, retryAfterMs: 5000 });
    const noBody = {
      ok: true,
      status: 200,
      headers: { get: () => null },
      body: null,
    } as unknown as Response;
    await expect(
      runEventStream(async () => noBody, jest.fn(), new AbortController().signal),
    ).resolves.toEqual({ kind: 'unsupported' });
  });
});

describe('backoffMs', () => {
  it('grows exponentially with jitter and caps at 30 s', () => {
    expect(backoffMs(0, () => 0)).toBe(500);
    expect(backoffMs(0, () => 1)).toBe(1000);
    expect(backoffMs(3, () => 1)).toBe(8000);
    expect(backoffMs(20, () => 1)).toBe(30_000);
  });
});
