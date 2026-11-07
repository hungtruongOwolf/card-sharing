import * as SecureStore from '../../platform/secureStore';

import { parseWidgetToken, type WidgetToken } from './widgetToken';

/**
 * Where the widget token lives. The app depends on this seam, not on a platform API: today it is the app's own
 * secure store; the native widgets (T-92..T-95) swap in storage their extension can read (iOS App Group Keychain,
 * Android EncryptedSharedPreferences) without touching the minting code.
 */
export interface WidgetTokenStore {
  load(): Promise<WidgetToken | null>;
  save(token: WidgetToken): Promise<void>;
  clear(): Promise<void>;
}

/** SecureStore keys allow only alphanumerics, `.`, `-` and `_`. Bump the suffix on a format change. */
export const WIDGET_TOKEN_KEY = 'cs.widget.token.v1';

const OPTIONS: SecureStore.SecureStoreOptions = {
  // The widget shows the latest card on the locked device too (AC-16.5, EM decision D3), so the token is
  // readable after the first unlock; never migrated to another device (it dies with this device's session).
  keychainAccessible: SecureStore.AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY,
};

const serialize = (token: WidgetToken) =>
  JSON.stringify({ token: token.token, expires_at: new Date(token.expiresAt).toISOString() });

function parseStored(raw: string | null): WidgetToken | null {
  if (raw === null) return null;
  try {
    return parseWidgetToken(JSON.parse(raw));
  } catch {
    return null;
  }
}

/** Keychain (iOS) / Keystore-backed (Android) storage via expo-secure-store; writes are serialized. */
export function createSecureWidgetTokenStore(): WidgetTokenStore {
  let queue: Promise<unknown> = Promise.resolve();
  const enqueue = <T>(op: () => Promise<T>): Promise<T> => {
    const next = queue.then(op, op);
    queue = next.catch(() => undefined);
    return next;
  };
  return {
    load: () =>
      enqueue(async () => parseStored(await SecureStore.getItemAsync(WIDGET_TOKEN_KEY, OPTIONS))),
    save: (token) =>
      enqueue(() => SecureStore.setItemAsync(WIDGET_TOKEN_KEY, serialize(token), OPTIONS)),
    clear: () => enqueue(() => SecureStore.deleteItemAsync(WIDGET_TOKEN_KEY, OPTIONS)),
  };
}

let device: WidgetTokenStore | null = null;

/** The app's one widget-token store (shared by the minting component and the sign-out cleanup). */
export function deviceWidgetTokenStore(): WidgetTokenStore {
  device ??= createSecureWidgetTokenStore();
  return device;
}
