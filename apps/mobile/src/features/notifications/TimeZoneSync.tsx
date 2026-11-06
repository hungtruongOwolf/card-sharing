import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef } from 'react';
import { AppState, type AppStateStatus } from 'react-native';

import { NotificationsError, type NotificationsApi } from './notificationsApi';
import { deviceTimeZone, syncTimeZone } from './timeZone';

export interface TimeZoneSyncProps {
  api: NotificationsApi;
  /** Seams for tests. */
  appState?: Pick<typeof AppState, 'currentState' | 'addEventListener'>;
  timeZone?: () => string | null;
}

/**
 * Sends the device's time zone while signed in (T-87): once when the account's session starts on this launch and
 * again whenever the app returns to the foreground in a different zone (travel, a manual change). A failed attempt
 * (offline, rate limit) is retried on the next foreground; a zone the server refuses (400) is not retried until the
 * zone changes again. Renders nothing; it remounts with the signed-in phase, so a new account syncs afresh.
 */
export function TimeZoneSync({
  api,
  appState = AppState,
  timeZone = deviceTimeZone,
}: TimeZoneSyncProps) {
  const queryClient = useQueryClient();
  const done = useRef<string | null>(null);
  const running = useRef(false);

  useEffect(() => {
    const check = () => {
      const zone = timeZone();
      if (!zone || zone === done.current || running.current) return;
      running.current = true;
      syncTimeZone(api, zone, queryClient)
        .then(
          () => {
            done.current = zone;
          },
          (error: unknown) => {
            if (error instanceof NotificationsError && error.kind === 'invalid') {
              done.current = zone;
            }
          },
        )
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
  }, [api, queryClient, appState, timeZone]);

  return null;
}
