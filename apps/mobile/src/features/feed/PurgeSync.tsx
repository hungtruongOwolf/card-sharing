import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef } from 'react';
import { AppState, type AppStateStatus } from 'react-native';

import type { CardsApi } from '../sharing/cardsApi';
import type { FeedCache } from './feedCache';
import { PURGE_CHECK_INTERVAL_MS, syncPurges } from './purges';

export interface PurgeSyncProps {
  api: CardsApi;
  cache: FeedCache;
  /** Seams for tests. */
  appState?: Pick<typeof AppState, 'currentState' | 'addEventListener'>;
  /** Calls `tick` every `ms` until the returned function is called. */
  every?: (tick: () => void, ms: number) => () => void;
}

const everyInterval = (tick: () => void, ms: number) => {
  const timer = setInterval(tick, ms);
  return () => clearInterval(timer);
};

/**
 * Runs {@link syncPurges} while signed in (T-83): on mount (app start), whenever the app comes to the foreground, and
 * every {@link PURGE_CHECK_INTERVAL_MS} while it stays there. Feed loads replace the offline copy with the server's
 * answer anyway; this covers the copy and the screens in between. Renders nothing.
 */
export function PurgeSync({
  api,
  cache,
  appState = AppState,
  every = everyInterval,
}: PurgeSyncProps) {
  const queryClient = useQueryClient();
  const running = useRef(false);

  useEffect(() => {
    const check = () => {
      if (running.current) return;
      running.current = true;
      syncPurges({ api, cache, queryClient })
        .catch(() => undefined)
        .finally(() => {
          running.current = false;
        });
    };
    let stop: (() => void) | null = null;
    const onChange = (state: AppStateStatus) => {
      stop?.();
      stop = null;
      if (state !== 'active') return;
      check();
      stop = every(check, PURGE_CHECK_INTERVAL_MS);
    };
    onChange(appState.currentState === 'background' ? 'background' : 'active');
    const subscription = appState.addEventListener('change', onChange);
    return () => {
      subscription.remove();
      stop?.();
    };
  }, [api, cache, queryClient, appState, every]);

  return null;
}
