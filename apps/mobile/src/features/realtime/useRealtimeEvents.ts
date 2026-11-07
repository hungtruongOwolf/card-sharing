import { useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';
import { AppState, type AppStateStatus } from 'react-native';

import { AuthError } from '../auth';
import {
  backoffMs,
  invalidationsFor,
  runEventStream,
  type FetchStream,
  type StreamEnd,
} from './eventStream';
import { FEED_QUERY_KEY } from '../feed/queryKeys';
import { ACTIVITY_QUERY_KEY } from '../activity/activityQuery';

/** Without a streaming fetch, the feed and activity are refetched this often while the app is open. */
export const POLL_FALLBACK_MS = 30_000;
/** A stream that lived this long resets the backoff. */
const HEALTHY_STREAM_MS = 60_000;
/** Reconnect delay after the server closed a stream on purpose (`reconnect`: max age, shutdown). */
const RECONNECT_MS = 500;

export interface RealtimeEventsOptions {
  /** Authorized streaming fetch (`AuthClient.fetchStream`). */
  fetchStream: FetchStream;
  /** Subscribe only while signed in and on the home phase. */
  enabled: boolean;
  /** Seams for tests. */
  appState?: Pick<typeof AppState, 'addEventListener' | 'currentState'>;
  now?: () => number;
}

const sessionEnded = (error: unknown): boolean =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

/**
 * Real-time updates (ADR-0010) while the app is in the foreground: one `GET /v1/events` stream, read with the
 * streaming fetch (`expo/fetch` on iOS and Android, the browser fetch on the web; `EventSource` cannot send the
 * bearer token). Every frame only invalidates the feed, activity or card queries, which refetch through the
 * normal routes. Errors back off exponentially (honouring `Retry-After` on 429/503); the stream closes in the
 * background. Where the runtime cannot stream a response body, it falls back to polling every 30 s.
 */
export function useRealtimeEvents({
  fetchStream,
  enabled,
  appState = AppState,
  now = Date.now,
}: RealtimeEventsOptions): void {
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!enabled) return undefined;
    let stopped = false;
    let foreground = appState.currentState === 'active' || appState.currentState == null;
    let controller: AbortController | null = null;
    let running = false;
    let poller: ReturnType<typeof setInterval> | null = null;
    let wake: (() => void) | null = null;

    const invalidate = (keys: readonly (readonly unknown[])[]) => {
      for (const queryKey of keys) void queryClient.invalidateQueries({ queryKey });
    };

    const sleep = (ms: number) =>
      new Promise<void>((resolve) => {
        const timer = setTimeout(() => {
          wake = null;
          resolve();
        }, ms);
        wake = () => {
          clearTimeout(timer);
          wake = null;
          resolve();
        };
      });

    const startPolling = () => {
      if (poller) return;
      invalidate([FEED_QUERY_KEY, ACTIVITY_QUERY_KEY]);
      poller = setInterval(() => {
        if (foreground) invalidate([FEED_QUERY_KEY, ACTIVITY_QUERY_KEY]);
      }, POLL_FALLBACK_MS);
    };

    const loop = async () => {
      if (running) return;
      running = true;
      let attempt = 0;
      while (!stopped && foreground && !poller) {
        controller = new AbortController();
        const startedAt = now();
        let end: StreamEnd | 'failed';
        try {
          end = await runEventStream(
            fetchStream,
            (frame) => invalidate(invalidationsFor(frame)),
            controller.signal,
          );
        } catch (error) {
          if (sessionEnded(error)) break;
          end = 'failed';
        }
        controller = null;
        if (stopped || !foreground) break;
        if (end !== 'failed' && end.kind === 'unsupported') {
          startPolling();
          break;
        }
        if (now() - startedAt >= HEALTHY_STREAM_MS) attempt = 0;
        let delay: number;
        if (end === 'failed') delay = backoffMs(attempt++);
        else if (end.kind === 'closed') delay = RECONNECT_MS;
        else delay = end.retryAfterMs ?? backoffMs(attempt++);
        await sleep(delay);
      }
      running = false;
    };

    const onChange = (status: AppStateStatus) => {
      const next = status === 'active';
      if (next === foreground) return;
      foreground = next;
      if (foreground) {
        if (!poller) void loop();
        else invalidate([FEED_QUERY_KEY, ACTIVITY_QUERY_KEY]);
      } else {
        // Background: close the stream (push covers a closed app).
        controller?.abort();
        wake?.();
      }
    };

    const subscription = appState.addEventListener('change', onChange);
    if (foreground) void loop();
    return () => {
      stopped = true;
      subscription.remove();
      controller?.abort();
      wake?.();
      if (poller) clearInterval(poller);
    };
  }, [enabled, fetchStream, queryClient, appState, now]);
}
