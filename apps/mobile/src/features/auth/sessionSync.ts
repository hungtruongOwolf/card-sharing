/**
 * Coordination between app instances that share one token store (ADR-0009). Browser tabs of one origin share
 * `localStorage`, but each tab holds the session in memory and refreshes on its own schedule. Refresh tokens are
 * single use and presenting a consumed one revokes the whole family, so two tabs rotating the same token sign the
 * user out everywhere (sprint 10 B5). Native has one app instance per device and uses none.
 */
export interface SessionSync {
  /** Runs `task` while no other instance runs one: a refresh, including its re-read of the shared store. */
  withRefreshLock<T>(task: () => Promise<T>): Promise<T>;
  /** Calls `listener` after another instance saved or cleared the stored session. Returns an unsubscribe. */
  subscribe(listener: () => void): () => void;
}

/** The part of the Web Locks API used (`navigator.locks`, exclusive mode). */
export interface LockManagerLike {
  request<T>(name: string, callback: () => Promise<T>): Promise<T>;
}

/** What a `storage` event carries that matters here: the changed key, or null for `localStorage.clear()`. */
export interface StorageChange {
  readonly key: string | null;
}

/** The part of `window` used: `storage` events, fired in every other tab of the origin after a write. */
export interface StorageEventsLike {
  addEventListener(type: 'storage', listener: (event: StorageChange) => void): void;
  removeEventListener(type: 'storage', listener: (event: StorageChange) => void): void;
}

/** One lock name per origin: every tab of the app refreshes under it. */
export const REFRESH_LOCK_NAME = 'cs.auth.refresh';

/**
 * Browser tabs: refreshes run under a Web Lock, and `storage` events for the session key tell a tab that another
 * one rotated, signed in or signed out. Without the Web Locks API (old browsers) the refresh runs unlocked; the
 * auth client still re-reads the store first, which closes most of the gap.
 */
export function createBrowserSessionSync(options: {
  locks: LockManagerLike | undefined;
  events: StorageEventsLike | undefined;
  /** The full `localStorage` key of the session. */
  storageKey: string;
}): SessionSync {
  const { locks, events, storageKey } = options;
  return {
    withRefreshLock: (task) => (locks ? locks.request(REFRESH_LOCK_NAME, task) : task()),
    subscribe(listener) {
      if (!events) return () => undefined;
      const onStorage = (event: StorageChange) => {
        if (event.key === null || event.key === storageKey) listener();
      };
      events.addEventListener('storage', onStorage);
      return () => events.removeEventListener('storage', onStorage);
    },
  };
}
