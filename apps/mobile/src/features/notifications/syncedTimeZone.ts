import { secureKeyValueStore, type KeyValueStore } from '../drafts/draftStore';

/**
 * The time zone this device last sent for the signed-in account (sprint 10 B7). Each launch compares the device zone
 * with it and only calls the server when the zone changed, so a web reload no longer spends a request of the
 * preferences limit. Account data like the rest of the device copies: cleared on sign-out (root layout).
 */
export interface SyncedTimeZoneStore {
  load(): Promise<string | null>;
  save(zone: string): Promise<void>;
  clear(): Promise<void>;
}

/** SecureStore entry names allow only alphanumerics, `.`, `-` and `_`. Bump the version on a format change. */
export const SYNCED_TIME_ZONE_ENTRY = 'cs.notifications.zone.v1';

/** Longest IANA zone name accepted back from storage (the API's own limit is the same order). */
const MAX_ZONE_LENGTH = 64;

export function createSyncedTimeZoneStore(
  kv: KeyValueStore = secureKeyValueStore(),
): SyncedTimeZoneStore {
  return {
    async load() {
      const value = await kv.getItem(SYNCED_TIME_ZONE_ENTRY).catch(() => null);
      return value && value.length <= MAX_ZONE_LENGTH ? value : null;
    },
    save: (zone) => kv.setItem(SYNCED_TIME_ZONE_ENTRY, zone),
    clear: () => kv.deleteItem(SYNCED_TIME_ZONE_ENTRY),
  };
}

let shared: SyncedTimeZoneStore | null = null;

/** The device's store (one instance for the app). */
export function deviceSyncedTimeZoneStore(): SyncedTimeZoneStore {
  shared ??= createSyncedTimeZoneStore();
  return shared;
}
