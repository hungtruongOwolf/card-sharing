import { readLocalPhotoBlob } from '../../platform/localPhoto.web';
import type { DraftStore } from './draftStore';
import { withPersistentPhotos, type PhotoBlobStore } from './persistentPhotos';

/**
 * Web (ADR-0009): draft photos are `blob:` URLs that die with the tab, so their bytes are kept in IndexedDB (same
 * origin, this browser profile only, never uploaded) and come back as fresh object URLs after a reload. Like the web
 * secure store, this is preview-grade storage: not encrypted at rest (security-baseline section 7).
 */
export const DRAFT_PHOTO_DB = 'cs-draft-photos';
const DB_VERSION = 1;
const STORE = 'photos';

const done = <T>(request: IDBRequest<T>) =>
  new Promise<T>((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });

/** One object store of photo Blobs keyed by draft id. */
export function indexedDbPhotoStore(
  factory: IDBFactory | undefined = globalThis.indexedDB,
): PhotoBlobStore {
  let db: Promise<IDBDatabase> | null = null;
  const open = () => {
    if (!factory) return Promise.reject(new Error('IndexedDB is not available'));
    db ??= new Promise<IDBDatabase>((resolve, reject) => {
      const request = factory.open(DRAFT_PHOTO_DB, DB_VERSION);
      request.onupgradeneeded = () => {
        if (!request.result.objectStoreNames.contains(STORE)) {
          request.result.createObjectStore(STORE);
        }
      };
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    }).catch((error: unknown) => {
      db = null;
      throw error;
    });
    return db;
  };
  const objects = async (mode: IDBTransactionMode) =>
    (await open()).transaction(STORE, mode).objectStore(STORE);

  return {
    get: async (draftId) => {
      const value: unknown = await done((await objects('readonly')).get(draftId));
      return value instanceof Blob ? value : null;
    },
    put: async (draftId, blob) => {
      await done((await objects('readwrite')).put(blob, draftId));
    },
    delete: async (draftId) => {
      await done((await objects('readwrite')).delete(draftId));
    },
  };
}

export function withDevicePhotos(store: DraftStore): DraftStore {
  return withPersistentPhotos(store, {
    blobs: indexedDbPhotoStore(),
    fetchBlob: (uri) => readLocalPhotoBlob(uri),
    createUrl: (blob) => URL.createObjectURL(blob),
    revokeUrl: (url) => URL.revokeObjectURL(url),
  });
}
