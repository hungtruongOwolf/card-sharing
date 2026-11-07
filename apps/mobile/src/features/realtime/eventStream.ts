import type { QueryKey } from '@tanstack/react-query';

import { ACTIVITY_QUERY_KEY } from '../activity/activityQuery';
import { cardQueryKey, FEED_QUERY_KEY } from '../feed/queryKeys';
import { createChunkDecoder, parseSse, type SseFrame } from './sse';

/** Query key of the replies to one of my cards (`GET /v1/cards/{id}/replies`). */
export const cardRepliesQueryKey = (id: string) => ['card-replies', id] as const;

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

const idOf = (frame: SseFrame, field: string): string | null => {
  const value = frame.data?.[field];
  return typeof value === 'string' && UUID.test(value) ? value : null;
};

/**
 * What to refetch for one frame. Frames carry ids only (ADR-0010): the app never shows anything from a frame, it
 * refetches through the normal read routes, which authorize every id again.
 */
export function invalidationsFor(frame: SseFrame): QueryKey[] {
  switch (frame.event) {
    // (Re)connected or events may have been missed: catch up (no replay log on the server).
    case 'ready':
    case 'resync':
      return [FEED_QUERY_KEY, ACTIVITY_QUERY_KEY];
    case 'card.delivered':
      return [FEED_QUERY_KEY, ACTIVITY_QUERY_KEY];
    case 'card.reply': {
      const original = idOf(frame, 'reply_to');
      return [
        FEED_QUERY_KEY,
        ACTIVITY_QUERY_KEY,
        ...(original ? [cardRepliesQueryKey(original)] : []),
      ];
    }
    case 'card.reaction': {
      const card = idOf(frame, 'card_id');
      return [ACTIVITY_QUERY_KEY, ...(card ? [cardQueryKey(card)] : [])];
    }
    default:
      return [];
  }
}

/** How a stream ended. */
export type StreamEnd =
  /** The server closed it (`reconnect`, end of body): reconnect soon. */
  | { readonly kind: 'closed' }
  /** Refused with an HTTP status (429, 503...); `retryAfterMs` from `Retry-After` when present. */
  | { readonly kind: 'refused'; readonly status: number; readonly retryAfterMs: number | null }
  /** The runtime gave no readable body (no streaming fetch): fall back to polling. */
  | { readonly kind: 'unsupported' };

export type FetchStream = (path: string, init?: RequestInit) => Promise<Response>;

interface ByteReader {
  read(): Promise<{ done: boolean; value?: Uint8Array }>;
  cancel?(): Promise<void>;
}

function retryAfterMs(response: Response): number | null {
  const raw = response.headers.get('Retry-After');
  const secs = raw === null ? NaN : Number(raw);
  return Number.isFinite(secs) && secs >= 0 ? Math.min(secs, 300) * 1000 : null;
}

/**
 * Opens `GET /v1/events` and hands every frame to `onFrame` until the stream ends or `signal` aborts. Network
 * failures reject (the caller backs off); auth failures reject with the auth client's `AuthError`.
 */
export async function runEventStream(
  fetchStream: FetchStream,
  onFrame: (frame: SseFrame) => void,
  signal: AbortSignal,
): Promise<StreamEnd> {
  const response = await fetchStream('/v1/events', {
    signal,
    headers: { Accept: 'text/event-stream' },
  });
  if (!response.ok) {
    return { kind: 'refused', status: response.status, retryAfterMs: retryAfterMs(response) };
  }
  const body = response.body as { getReader?: () => ByteReader } | null;
  if (!body || typeof body.getReader !== 'function') return { kind: 'unsupported' };
  const reader = body.getReader();
  const decode = createChunkDecoder();
  let buffer = '';
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return { kind: 'closed' };
      if (!value) continue;
      const parsed = parseSse(buffer + decode(value));
      buffer = parsed.rest;
      // A runaway frame without a terminator: drop it rather than grow without bound.
      if (buffer.length > 64 * 1024) buffer = '';
      for (const frame of parsed.frames) {
        onFrame(frame);
        if (frame.event === 'reconnect') return { kind: 'closed' };
      }
    }
  } finally {
    void reader.cancel?.().catch(() => undefined);
  }
}

/** Exponential backoff with jitter: 1 s, 2 s, 4 s ... capped at 30 s. */
export function backoffMs(attempt: number, random: () => number = Math.random): number {
  const base = Math.min(30_000, 1000 * 2 ** Math.min(attempt, 5));
  return Math.round(base / 2 + random() * (base / 2));
}
