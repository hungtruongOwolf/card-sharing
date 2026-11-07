import type { DraftStore } from './draftStore';

/**
 * Native: a draft's photo is a file in app storage that outlives the app session, so the store is used as is. The
 * web variant (`draftPhotos.web.ts`) keeps the photo bytes in IndexedDB.
 */
export function withDevicePhotos(store: DraftStore): DraftStore {
  return store;
}
