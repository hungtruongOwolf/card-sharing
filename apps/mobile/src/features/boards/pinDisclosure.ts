import { secureKeyValueStore, type KeyValueStore } from '../drafts/draftStore';

/**
 * The one-time explanation before I pin my own card to a shared board (prd-mvp AC-19.5: "Everyone on this board can
 * see this card"). Remembered on the device for the signed-in account and forgotten on sign-out, so another account
 * on the same phone sees it again (shown more often, never less).
 */
export interface PinDisclosure {
  seen(): Promise<boolean>;
  markSeen(): Promise<void>;
  clear(): Promise<void>;
}

export const PIN_DISCLOSURE_KEY = 'cs.boards.v1.disclosure';

export function createPinDisclosure(kv: KeyValueStore = secureKeyValueStore()): PinDisclosure {
  return {
    seen: async () => (await kv.getItem(PIN_DISCLOSURE_KEY).catch(() => null)) === '1',
    markSeen: () => kv.setItem(PIN_DISCLOSURE_KEY, '1'),
    clear: () => kv.deleteItem(PIN_DISCLOSURE_KEY),
  };
}

/** In memory, for tests and previews. */
export function memoryPinDisclosure(initiallySeen = false): PinDisclosure {
  let seen = initiallySeen;
  return {
    seen: async () => seen,
    markSeen: async () => {
      seen = true;
    },
    clear: async () => {
      seen = false;
    },
  };
}
