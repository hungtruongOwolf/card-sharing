import { WEB_KEY_PREFIX } from '../../platform/secureStore.web';
import { createBrowserSessionSync, type LockManagerLike, type SessionSync } from './sessionSync';
import { SESSION_KEY } from './tokenStore';

/** Web: tabs of the origin share the session in `localStorage` (platform/secureStore.web.ts) and coordinate refreshes. */
export function appSessionSync(): SessionSync | undefined {
  const scope = globalThis as {
    navigator?: { locks?: LockManagerLike };
    addEventListener?: unknown;
  };
  return createBrowserSessionSync({
    locks: scope.navigator?.locks,
    events:
      typeof scope.addEventListener === 'function'
        ? (globalThis as unknown as Parameters<typeof createBrowserSessionSync>[0]['events'])
        : undefined,
    storageKey: WEB_KEY_PREFIX + SESSION_KEY,
  });
}
