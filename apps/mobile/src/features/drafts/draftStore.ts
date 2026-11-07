import * as SecureStore from '../../platform/secureStore';

import {
  MAX_EFFECTS,
  migrateId,
  registry as shippedRegistry,
  RETIRED,
  type ParamValue,
  type Registry,
} from '@card-sharing/effects-registry';

import { sanitizeText } from '../../text';
import { LOCAL_CARD_SCHEMA, type LocalCard } from '../cards/localCard';
import type { CardDraft, StyleChoice } from '../editor/editorState';
import { parseEditorPhoto } from '../editor/editorRoute';

/**
 * Private drafts (T-68, US-9): unsent cards kept on this device only. A draft is its local card record (photo
 * reference + style choices, ~1 KB), never a render, so it stays editable and holds nothing new to protect beyond
 * the photo the user already has. Nothing here talks to the network.
 *
 * Storage: expo-secure-store (Keychain / Keystore-encrypted), `WHEN_UNLOCKED_THIS_DEVICE_ONLY`, so records are
 * encrypted at rest, unreadable while the phone is locked, never migrated through an iCloud/iTunes backup, and the
 * expo-secure-store config plugin keeps them out of Android Auto Backup (security-baseline: no readable cloud copy).
 * SQLite (the ticket's plan) is not a dependency yet; see the follow-up issue.
 */
export interface DraftStore {
  /** Newest first; unreadable records are skipped. */
  list(): Promise<LocalCard[]>;
  get(id: string): Promise<LocalCard | null>;
  /** Creates or replaces. Throws {@link DraftStoreError} when full or the record is too large. */
  save(card: LocalCard): Promise<void>;
  remove(id: string): Promise<void>;
}

/** Minimal async key-value seam over SecureStore, so tests and other backends can plug in. */
export interface KeyValueStore {
  getItem(key: string): Promise<string | null>;
  setItem(key: string, value: string): Promise<void>;
  deleteItem(key: string): Promise<void>;
}

/** SecureStore keys allow only alphanumerics, `.`, `-` and `_`. Bump the version on a format change. */
export const DRAFT_INDEX_KEY = 'cs.drafts.index.v1';
export const draftKey = (id: string) => `cs.drafts.item.v1.${id}`;

/** Drafts kept at most (the index of ids stays under the SecureStore value size). */
export const MAX_DRAFTS = 40;
/** expo-secure-store may fail above 2048 bytes per value on Android; refuse instead of failing later. */
export const MAX_DRAFT_BYTES = 2048;

const ID_PATTERN = /^[A-Za-z0-9-]{1,64}$/;

export type DraftStoreErrorReason = 'full' | 'tooLarge' | 'invalid';

export class DraftStoreError extends Error {
  override readonly name = 'DraftStoreError';
  constructor(readonly reason: DraftStoreErrorReason) {
    super(`draft store: ${reason}`);
  }
}

const SECURE_OPTIONS: SecureStore.SecureStoreOptions = {
  keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
};

export function secureKeyValueStore(): KeyValueStore {
  return {
    getItem: (key) => SecureStore.getItemAsync(key, SECURE_OPTIONS),
    setItem: (key, value) => SecureStore.setItemAsync(key, value, SECURE_OPTIONS),
    deleteItem: (key) => SecureStore.deleteItemAsync(key, SECURE_OPTIONS),
  };
}

export function memoryKeyValueStore(): KeyValueStore & { items: Map<string, string> } {
  const items = new Map<string, string>();
  return {
    items,
    getItem: async (key) => items.get(key) ?? null,
    setItem: async (key, value) => {
      items.set(key, value);
    },
    deleteItem: async (key) => {
      items.delete(key);
    },
  };
}

/** UTF-8 byte length without TextEncoder (not guaranteed on every Hermes build). */
export function utf8Length(text: string): number {
  let bytes = 0;
  for (const char of text) {
    const code = char.codePointAt(0) ?? 0;
    bytes += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
  }
  return bytes;
}

export function createDraftStore(kv: KeyValueStore = secureKeyValueStore()): DraftStore {
  // Serialized like the token store: an index update never races another one.
  let queue: Promise<unknown> = Promise.resolve();
  const enqueue = <T>(op: () => Promise<T>): Promise<T> => {
    const next = queue.then(op, op);
    queue = next.catch(() => undefined);
    return next;
  };

  const readIndex = async (): Promise<string[]> => parseIndex(await kv.getItem(DRAFT_INDEX_KEY));
  const readCard = async (id: string) => parseLocalCard(await kv.getItem(draftKey(id)), id);

  return {
    list: () =>
      enqueue(async () => {
        const cards = await Promise.all((await readIndex()).map(readCard));
        return cards
          .filter((card): card is LocalCard => card !== null)
          .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
      }),
    get: (id) => enqueue(async () => (ID_PATTERN.test(id) ? readCard(id) : null)),
    save: (card) =>
      enqueue(async () => {
        if (!ID_PATTERN.test(card.id)) throw new DraftStoreError('invalid');
        const value = JSON.stringify(card);
        if (utf8Length(value) > MAX_DRAFT_BYTES) throw new DraftStoreError('tooLarge');
        const index = await readIndex();
        const known = index.includes(card.id);
        if (!known && index.length >= MAX_DRAFTS) throw new DraftStoreError('full');
        await kv.setItem(draftKey(card.id), value);
        if (!known) await kv.setItem(DRAFT_INDEX_KEY, JSON.stringify([...index, card.id]));
      }),
    remove: (id) =>
      enqueue(async () => {
        if (!ID_PATTERN.test(id)) return;
        const index = await readIndex();
        await kv.deleteItem(draftKey(id));
        if (index.includes(id)) {
          await kv.setItem(DRAFT_INDEX_KEY, JSON.stringify(index.filter((known) => known !== id)));
        }
      }),
  };
}

function parseIndex(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return [
      ...new Set(
        parsed.filter((id): id is string => typeof id === 'string' && ID_PATTERN.test(id)),
      ),
    ];
  } catch {
    return [];
  }
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const isParamValue = (value: unknown): value is ParamValue =>
  typeof value === 'boolean' ||
  typeof value === 'string' ||
  (typeof value === 'number' && Number.isFinite(value));

function parseChoice(value: unknown): StyleChoice | null {
  if (!isRecord(value) || typeof value.id !== 'string' || !Number.isSafeInteger(value.version)) {
    return null;
  }
  if (!isRecord(value.params) || !Object.values(value.params).every(isParamValue)) return null;
  return {
    id: value.id,
    version: value.version as number,
    params: value.params as Record<string, ParamValue>,
  };
}

/**
 * A stored choice after the registry ledger's migrations (T-124): unchanged while its id ships, its replacement
 * (default params, current version) when the id was retired into another, or null when it was retired without one.
 */
export function migrateChoice(
  choice: StyleChoice,
  registry: Registry = shippedRegistry,
  retired: Readonly<Record<string, string | null>> = RETIRED,
): StyleChoice | null {
  const id = migrateId(choice.id, retired);
  if (id === choice.id) return choice;
  const entry = id === null ? undefined : registry.get(id);
  return entry ? { id: entry.id, version: entry.version, params: {} } : null;
}

const isIsoDate = (value: unknown): value is string =>
  typeof value === 'string' && !Number.isNaN(Date.parse(value));

/**
 * Reads a stored record defensively (storage is outside the type system; a downgrade or corruption must not crash
 * the tray). The photo must still be a photo the app holds (`parseEditorPhoto`); params are clamped later by the renderers.
 */
export function parseLocalCard(raw: string | null, expectedId?: string): LocalCard | null {
  if (!raw) return null;
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return null;
  }
  if (!isRecord(value) || value.schema !== LOCAL_CARD_SCHEMA) return null;
  if (typeof value.id !== 'string' || !ID_PATTERN.test(value.id)) return null;
  if (expectedId !== undefined && value.id !== expectedId) return null;
  if (!isIsoDate(value.createdAt) || !isIsoDate(value.updatedAt)) return null;
  const photo = isRecord(value.photo)
    ? parseEditorPhoto({
        uri: value.photo.uri,
        width: String(value.photo.width),
        height: String(value.photo.height),
        origin: value.photo.origin,
      })
    : null;
  if (!photo || !isRecord(value.draft)) return null;
  const cut = parseChoice(value.draft.cut);
  const finish = value.draft.finish === null ? null : parseChoice(value.draft.finish);
  if (!cut || (value.draft.finish !== null && !finish)) return null;
  // Drafts saved before effects existed (T-114) have no list: no effects.
  const storedEffects = value.draft.effects ?? [];
  if (!Array.isArray(storedEffects) || storedEffects.length > MAX_EFFECTS) return null;
  const effects = storedEffects.map(parseChoice);
  if (effects.some((choice) => choice === null)) return null;
  if (typeof value.draft.wholePhoto !== 'boolean' || typeof value.draft.caption !== 'string') {
    return null;
  }
  const migrate = (choice: StyleChoice) => migrateChoice(choice);
  const draft: CardDraft = {
    // A retired cut without a replacement stays as stored: the editor falls back to its default.
    cut: migrate(cut) ?? cut,
    finish: finish ? migrate(finish) : null,
    effects: (effects as StyleChoice[]).flatMap((choice) => migrate(choice) ?? []),
    wholePhoto: value.draft.wholePhoto,
    caption: sanitizeText(value.draft.caption),
  };
  return {
    id: value.id,
    schema: LOCAL_CARD_SCHEMA,
    photo,
    draft,
    createdAt: value.createdAt,
    updatedAt: value.updatedAt,
  };
}
