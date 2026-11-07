import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef } from 'react';
import { AppState, type AppStateStatus } from 'react-native';

import { NotificationsError, type NotificationsApi } from './notificationsApi';
import { deviceSyncedTimeZoneStore, type SyncedTimeZoneStore } from './syncedTimeZone';
import { deviceTimeZone, syncTimeZone } from './timeZone';

export interface TimeZoneSyncProps {
  api: NotificationsApi;
  /** The zone this device last sent for the account; defaults to the device store (cleared on sign-out). */
  store?: SyncedTimeZoneStore;
  /** Seams for tests. */
  appState?: Pick<typeof AppState, 'currentState' | 'addEventListener'>;
  timeZone?: () => string | null;
  now?: () => number;
}

/** Wait after a 429 that carried no `Retry-After`. */
export const RATE_LIMIT_BACKOFF_MS = 60_000;

/**
 * Sends the device's time zone while signed in (T-87): when the session starts on this launch and whenever the app
 * returns to the foreground, but only if the zone differs from the one this device last sent (B7: a web reload used
 * to read the preferences every time and ran into the endpoint's rate limit). A failed attempt (offline) is retried
 * on the next foreground; after a 429 nothing is sent before its `Retry-After` has passed; a zone the server refuses
 * (400) is not retried until the zone changes again. Renders nothing; it remounts with the signed-in phase.
 */
export function TimeZoneSync({
  api,
  store = deviceSyncedTimeZoneStore(),
  appState = AppState,
  timeZone = deviceTimeZone,
  now = Date.now,
}: TimeZoneSyncProps) {
  const queryClient = useQueryClient();
  const done = useRef<string | null>(null);
  const running = useRef(false);
  const blockedUntil = useRef(0);

  useEffect(() => {
    const sync = async (zone: string) => {
      if ((await store.load()) === zone) {
        done.current = zone;
        return;
      }
      try {
        await syncTimeZone(api, zone, queryClient);
        done.current = zone;
        await store.save(zone).catch(() => undefined);
      } catch (error) {
        if (!(error instanceof NotificationsError)) return;
        if (error.kind === 'invalid') done.current = zone;
        if (error.kind === 'rate_limited') {
          blockedUntil.current = now() + (error.retryAfterMs ?? RATE_LIMIT_BACKOFF_MS);
        }
      }
    };
    const check = () => {
      const zone = timeZone();
      if (!zone || zone === done.current || running.current) return;
      if (now() < blockedUntil.current) return;
      running.current = true;
      sync(zone)
        .catch(() => undefined)
        .finally(() => {
          running.current = false;
        });
    };
    const onChange = (state: AppStateStatus) => {
      if (state === 'active') check();
    };
    onChange(appState.currentState === 'background' ? 'background' : 'active');
    const subscription = appState.addEventListener('change', onChange);
    return () => subscription.remove();
  }, [api, store, queryClient, appState, timeZone, now]);

  return null;
}
