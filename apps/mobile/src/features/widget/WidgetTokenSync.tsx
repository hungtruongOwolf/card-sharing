import { useEffect, useRef } from 'react';
import { AppState, Platform, type AppStateStatus, type PlatformOSType } from 'react-native';

import type { WidgetTokenApi } from './widgetToken';
import type { WidgetTokenStore } from './widgetTokenStore';

export interface WidgetTokenSyncProps {
  api: WidgetTokenApi;
  store: WidgetTokenStore;
  /** Seams for tests. */
  appState?: Pick<typeof AppState, 'currentState' | 'addEventListener'>;
  now?: () => number;
  os?: PlatformOSType;
}

/** Platforms with a home-screen widget to feed (T-92 iOS, T-93 Android). The web target has none (ADR-0009). */
export const hasHomeScreenWidget = (os: PlatformOSType = Platform.OS) =>
  os === 'ios' || os === 'android';

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
 *
 * Does nothing on the web (sprint 10 W1): there is no widget to read the token, and every web sign-in minting one
 * spent the per-device daily limit, which falls back to the IP for web sessions (no device id).
 */
export function WidgetTokenSync({
  api,
  store,
  appState = AppState,
  now = Date.now,
  os = Platform.OS,
}: WidgetTokenSyncProps) {
  const running = useRef(false);
  const enabled = hasHomeScreenWidget(os);

  useEffect(() => {
    if (!enabled) return;
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
  }, [api, store, appState, now, enabled]);

  return null;
}
