import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react-native';
import type { ReactNode } from 'react';
import type { AppStateStatus } from 'react-native';

import { ACTIVITY_QUERY_KEY } from '../../activity/activityQuery';
import { AuthError } from '../../auth';
import { FEED_QUERY_KEY } from '../../feed/queryKeys';
import type { FetchStream } from '../eventStream';
import { POLL_FALLBACK_MS, useRealtimeEvents } from '../useRealtimeEvents';

const CARD = 'c0000000-0000-4000-8000-000000000001';

function fakeAppState(initial: AppStateStatus = 'active') {
  let listener: ((status: AppStateStatus) => void) | null = null;
  return {
    currentState: initial,
    addEventListener: (_: 'change', fn: (status: AppStateStatus) => void) => {
      listener = fn;
      return { remove: () => (listener = null) };
    },
    emit: (status: AppStateStatus) => listener?.(status),
  };
}

/** A body that yields `chunks` and then stays open until aborted. */
function openStream(chunks: string[], signal: AbortSignal): Response {
  const queue = chunks.map((text) => Uint8Array.from(text, (c) => c.charCodeAt(0)));
  return {
    ok: true,
    status: 200,
    headers: { get: () => null },
    body: {
      getReader: () => ({
        read: () => {
          const value = queue.shift();
          if (value) return Promise.resolve({ done: false, value });
          return new Promise((resolve) =>
            signal.addEventListener('abort', () => resolve({ done: true })),
          );
        },
        cancel: async () => undefined,
      }),
    },
  } as unknown as Response;
}

async function setup(fetchStream: FetchStream, appState = fakeAppState()) {
  const client = new QueryClient();
  const invalidate = jest.spyOn(client, 'invalidateQueries');
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const hook = await renderHook(
    () =>
      useRealtimeEvents({
        fetchStream,
        enabled: true,
        appState: appState as unknown as Parameters<typeof useRealtimeEvents>[0]['appState'],
      }),
    { wrapper },
  );
  const keys = () => invalidate.mock.calls.map(([filters]) => filters?.queryKey);
  return { hook, keys, appState };
}

afterEach(() => jest.useRealTimers());

describe('useRealtimeEvents', () => {
  it('refetches on ready and on each event, and closes the stream in the background', async () => {
    const signals: AbortSignal[] = [];
    const fetchStream: FetchStream = jest.fn(async (_path, init) => {
      signals.push(init!.signal!);
      return openStream(
        ['event: ready\ndata: {}\n\n', `event: card.delivered\ndata: {"card_id":"${CARD}"}\n\n`],
        init!.signal!,
      );
    });
    const { keys, appState, hook } = await setup(fetchStream);
    await waitFor(() => expect(keys().filter((k) => k === FEED_QUERY_KEY)).toHaveLength(2));
    expect(keys()).toContain(ACTIVITY_QUERY_KEY);
    await act(async () => appState.emit('background'));
    expect(signals[0]?.aborted).toBe(true);
    await hook.unmount();
  });

  it('stops for good when the session ended', async () => {
    const fetchStream: FetchStream = jest.fn(async () => {
      throw new AuthError('session_ended', 401, 'SESSION_REVOKED');
    });
    const { hook } = await setup(fetchStream);
    await waitFor(() => expect(fetchStream).toHaveBeenCalledTimes(1));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });
    expect(fetchStream).toHaveBeenCalledTimes(1);
    await hook.unmount();
  });

  it('falls back to polling when the runtime cannot stream a body', async () => {
    jest.useFakeTimers();
    const fetchStream: FetchStream = jest.fn(
      async () =>
        ({
          ok: true,
          status: 200,
          headers: { get: () => null },
          body: null,
        }) as unknown as Response,
    );
    const { keys, hook } = await setup(fetchStream);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    const before = keys().filter((k) => k === FEED_QUERY_KEY).length;
    expect(before).toBeGreaterThanOrEqual(1);
    await act(async () => {
      jest.advanceTimersByTime(POLL_FALLBACK_MS);
    });
    expect(keys().filter((k) => k === FEED_QUERY_KEY).length).toBe(before + 1);
    expect(fetchStream).toHaveBeenCalledTimes(1);
    await hook.unmount();
  });
});
