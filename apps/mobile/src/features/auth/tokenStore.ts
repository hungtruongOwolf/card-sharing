import * as SecureStore from '../../platform/secureStore';

import { parseStoredSession, serializeSession, type Session } from './session';

/** Where the device session is persisted. The auth client depends on this seam, not on a platform API. */
export interface TokenStore {
  load(): Promise<Session | null>;
  save(session: Session): Promise<void>;
  clear(): Promise<void>;
}

/** SecureStore keys allow only alphanumerics, `.`, `-` and `_`. Bump the suffix on a format change. */
export const SESSION_KEY = 'cs.auth.session.v1';

const OPTIONS: SecureStore.SecureStoreOptions = {
  // Keychain item readable after the first unlock (background refresh works) and never migrated to
  // another device through a backup: a session belongs to the device it was issued to (ADR-0004).
  // Android: stored with a Keystore key; the config plugin excludes SecureStore data from Auto Backup.
  keychainAccessible: SecureStore.AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY,
};

/**
 * Keychain (iOS) / Keystore-backed (Android) session storage via expo-secure-store. Never AsyncStorage.
 * Writes are serialized so a `clear()` issued after a `save()` always wins.
 */
export function createSecureTokenStore(): TokenStore {
  let queue: Promise<unknown> = Promise.resolve();
  const enqueue = <T>(op: () => Promise<T>): Promise<T> => {
    const next = queue.then(op, op);
    queue = next.catch(() => undefined);
    return next;
  };

  return {
    load: () =>
      enqueue(async () => parseStoredSession(await SecureStore.getItemAsync(SESSION_KEY, OPTIONS))),
    save: (session) =>
      enqueue(() => SecureStore.setItemAsync(SESSION_KEY, serializeSession(session), OPTIONS)),
    clear: () => enqueue(() => SecureStore.deleteItemAsync(SESSION_KEY, OPTIONS)),
  };
}
