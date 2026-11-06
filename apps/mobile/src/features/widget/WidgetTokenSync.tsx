import { useEffect, useRef } from 'react';
import { AppState, type AppStateStatus } from 'react-native';

import type { WidgetTokenApi } from './widgetToken';
import type { WidgetTokenStore } from './widgetTokenStore';

export interface WidgetTokenSyncProps {
  api: WidgetTokenApi;
  store: WidgetTokenStore;
  /** Seams for tests. */
  appState?: Pick<typeof AppState, 'currentState' | 'addEventListener'>;
  now?: () => number;
}

/**
 * Mints and stores a token unless a stored one is still valid; true when it stored a new one. Renewing early would
 * gain nothing: the server gives the token the session's absolute expiry, and a new session starts with an empty
 * store (sign-out clears it).
 * `stillSignedIn` is asked after the network call: a token minted while the device signed out is dropped, so it
 * cannot land in the store after the sign-out cleared it.
 */
export async function ensureWidgetToken(
  api: WidgetTokenApi,
  store: WidgetTokenStore,
  now: number,
  stillSignedIn: () => boolean = () => true,
): Promise<boolean> {
  const stored = await store.load();
  if (stored && stored.expiresAt > now) return false;
  const token = await api.mint();
  if (!stillSignedIn()) return false;
  await store.save(token);
  return true;
}

/**
 * Makes sure a widget token for this session is stored while signed in (T-91, US-16 AC-16.5): when the session
 * starts on this launch and on every return to the foreground, it mints one (`POST /v1/me/widget-token`, 20 a day
 * per device) only if none is stored or the stored one has expired. Minting replaces the session's
 * earlier token on the server. Failures (offline, rate limit) retry on the next foreground. Sign-out clears the
 * store (root layout); this component unmounts with the signed-in phase. Renders nothing.
 */
export function WidgetTokenSync({
  api,
  store,
  appState = AppState,
  now = Date.now,
}: WidgetTokenSyncProps) {
  const running = useRef(false);

  useEffect(() => {
    let mounted = true;
    const check = () => {
      if (running.current || !mounted) return;
      running.current = true;
      ensureWidgetToken(api, store, now(), () => mounted)
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
    return () => {
      mounted = false;
      subscription.remove();
    };
  }, [api, store, appState, now]);

  return null;
}
