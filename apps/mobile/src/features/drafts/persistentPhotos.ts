import type { LocalCard } from '../cards/localCard';
import type { DraftStore } from './draftStore';

/**
 * Keeps draft photos across reloads on the web (ADR-0009). A web photo is a `blob:` URL that dies with the tab, so the
 * draft record alone (a photo reference) would come back without its photo. This wrapper copies the photo bytes
 * into a blob store keyed by draft id the first time a draft is saved with a new `blob:` photo, hands out a fresh
 * object URL when a draft is read in a later session, and deletes the bytes with the draft. Native photos are files
 * in app storage and never go through here.
 */
export interface PhotoBlobStore {
  get(draftId: string): Promise<Blob | null>;
  put(draftId: string, blob: Blob): Promise<void>;
  delete(draftId: string): Promise<void>;
}

export interface PersistentPhotoDeps {
  blobs: PhotoBlobStore;
  /** Reads the bytes behind a live `blob:` URL. */
  fetchBlob: (uri: string) => Promise<Blob>;
  createUrl: (blob: Blob) => string;
  revokeUrl: (url: string) => void;
  /** Larger photos are not kept (the draft still saves; after a reload it has no photo, as before). */
  maxBytes?: number;
}

/** A phone photo is a few MB; this leaves room for a large library pick and caps 40 drafts well under quota. */
export const MAX_DRAFT_PHOTO_BYTES = 12 * 1024 * 1024;

const isBlobUri = (uri: string) => uri.startsWith('blob:');

export function withPersistentPhotos(
  store: DraftStore,
  { blobs, fetchBlob, createUrl, revokeUrl, maxBytes = MAX_DRAFT_PHOTO_BYTES }: PersistentPhotoDeps,
): DraftStore {
  /** Draft id → the photo URL that is live in this tab and whose bytes are stored (or too large to store). */
  const live = new Map<string, string>();
  /** Draft id → an object URL this wrapper created (revoked when the draft goes). */
  const created = new Map<string, string>();

  const keep = async (card: LocalCard) => {
    const { uri } = card.photo;
    if (!isBlobUri(uri) || live.get(card.id) === uri) return;
    try {
      const blob = await fetchBlob(uri);
      if (blob.size > 0 && blob.size <= maxBytes && blob.type.startsWith('image/')) {
        await blobs.put(card.id, blob);
      }
      live.set(card.id, uri);
    } catch {
      // Best effort: a full quota or a dead URL never blocks saving the draft itself.
    }
  };

  const restore = async (card: LocalCard | null): Promise<LocalCard | null> => {
    if (!card || !isBlobUri(card.photo.uri)) return card;
    const current = live.get(card.id);
    if (current) return current === card.photo.uri ? card : withUri(card, current);
    const blob = await blobs.get(card.id).catch(() => null);
    if (!blob) return card;
    const url = createUrl(blob);
    live.set(card.id, url);
    created.set(card.id, url);
    return withUri(card, url);
  };

  return {
    list: async () =>
      (await Promise.all((await store.list()).map(restore))).filter(
        (card): card is LocalCard => card !== null,
      ),
    get: async (id) => restore(await store.get(id)),
    save: async (card) => {
      await keep(card);
      await store.save(card);
    },
    remove: async (id) => {
      await store.remove(id);
      live.delete(id);
      const url = created.get(id);
      if (url) {
        created.delete(id);
        revokeUrl(url);
      }
      await blobs.delete(id).catch(() => undefined);
    },
  };
}

const withUri = (card: LocalCard, uri: string): LocalCard => ({
  ...card,
  photo: { ...card.photo, uri },
});
