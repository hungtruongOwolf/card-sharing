import { secureKeyValueStore, type KeyValueStore } from '../drafts/draftStore';

/**
 * Device memory for public links (T-134): whether the one-time link-preview disclosure was shown (AC-24.7) and
 * whether the server refused links for this account (AC-24.6, 403 `POLICY_DISALLOWED`), so the share-link action
 * stays hidden afterwards. `/v1/me` does not expose the region rule yet, so the first refusal is how the app learns
 * it. Both are forgotten on sign-out with the other board state (shown more often, never less).
 */
export interface PublicLinkPrefs {
  disclosureSeen(): Promise<boolean>;
  markDisclosureSeen(): Promise<void>;
  disallowed(): Promise<boolean>;
  markDisallowed(): Promise<void>;
  clear(): Promise<void>;
}

export const LINK_DISCLOSURE_KEY = 'cs.links.v1.disclosure';
export const LINKS_DISALLOWED_KEY = 'cs.links.v1.disallowed';

export function createPublicLinkPrefs(kv: KeyValueStore = secureKeyValueStore()): PublicLinkPrefs {
  const flag = async (key: string) => (await kv.getItem(key).catch(() => null)) === '1';
  return {
    disclosureSeen: () => flag(LINK_DISCLOSURE_KEY),
    markDisclosureSeen: () => kv.setItem(LINK_DISCLOSURE_KEY, '1'),
    disallowed: () => flag(LINKS_DISALLOWED_KEY),
    markDisallowed: () => kv.setItem(LINKS_DISALLOWED_KEY, '1'),
    clear: async () => {
      await kv.deleteItem(LINK_DISCLOSURE_KEY);
      await kv.deleteItem(LINKS_DISALLOWED_KEY);
    },
  };
}

/** In memory, for tests and previews. */
export function memoryPublicLinkPrefs(
  initial: { disclosureSeen?: boolean; disallowed?: boolean } = {},
): PublicLinkPrefs {
  let seen = initial.disclosureSeen ?? false;
  let refused = initial.disallowed ?? false;
  return {
    disclosureSeen: async () => seen,
    markDisclosureSeen: async () => {
      seen = true;
    },
    disallowed: async () => refused,
    markDisallowed: async () => {
      refused = true;
    },
    clear: async () => {
      seen = false;
      refused = false;
    },
  };
}
