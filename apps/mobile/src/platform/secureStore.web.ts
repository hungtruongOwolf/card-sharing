import type * as Native from 'expo-secure-store';

/**
 * Web stand-in for expo-secure-store (ADR-0009). The browser has no keychain: values live in `localStorage`, which
 * is NOT encrypted at rest and is readable by any script running on the origin (an XSS reads the session). This is
 * accepted only because the web build is a preview target, never a launch target; a web launch needs HttpOnly
 * cookie sessions instead (security-baseline.md). The keychain options are accepted and ignored.
 *
 * When `localStorage` is unavailable (some private modes, storage disabled) values are kept in memory for the tab,
 * so the app still works and simply forgets everything on reload.
 */
export type SecureStoreOptions = Native.SecureStoreOptions;

/** Same values as expo-secure-store's constants; meaningless on the web. */
export const AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY: Native.KeychainAccessibilityConstant = 1;
export const WHEN_UNLOCKED_THIS_DEVICE_ONLY: Native.KeychainAccessibilityConstant = 6;

/** Namespaces the app's entries in the origin's storage. */
export const WEB_KEY_PREFIX = 'cs.secure.';

/** Keys follow expo-secure-store's rule, so a key that works on the web also works on a phone. */
const VALID_KEY = /^[\w.-]+$/;

interface KeyValueBackend {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function memoryBackend(): KeyValueBackend {
  const items = new Map<string, string>();
  return {
    getItem: (key) => items.get(key) ?? null,
    setItem: (key, value) => void items.set(key, value),
    removeItem: (key) => void items.delete(key),
  };
}

function browserBackend(): KeyValueBackend {
  try {
    const storage = globalThis.localStorage;
    const probe = `${WEB_KEY_PREFIX}probe`;
    storage.setItem(probe, '1');
    storage.removeItem(probe);
    return storage;
  } catch {
    return memoryBackend();
  }
}

let backend: KeyValueBackend | null = null;
const store = (): KeyValueBackend => (backend ??= browserBackend());

/** Test hook: use `next` (or detect again when null). */
export function setWebBackendForTests(next: KeyValueBackend | null): void {
  backend = next;
}

function storageKey(key: string): string {
  if (!VALID_KEY.test(key)) throw new Error(`Invalid secure store key: ${key}`);
  return WEB_KEY_PREFIX + key;
}

export async function getItemAsync(
  key: string,
  _options?: SecureStoreOptions,
): Promise<string | null> {
  return store().getItem(storageKey(key));
}

export async function setItemAsync(
  key: string,
  value: string,
  _options?: SecureStoreOptions,
): Promise<void> {
  store().setItem(storageKey(key), value);
}

export async function deleteItemAsync(key: string, _options?: SecureStoreOptions): Promise<void> {
  store().removeItem(storageKey(key));
}
