import { withDevicePhotos } from './draftPhotos';
import { createDraftStore, type DraftStore } from './draftStore';

export {
  createDraftStore,
  DRAFT_INDEX_KEY,
  draftKey,
  DraftStoreError,
  MAX_DRAFT_BYTES,
  MAX_DRAFTS,
  memoryKeyValueStore,
  parseLocalCard,
  secureKeyValueStore,
  type DraftStore,
  type KeyValueStore,
} from './draftStore';
export { DraftsScreen, type DraftsScreenProps } from './DraftsScreen';
export {
  isLowStorage,
  LOW_STORAGE_BYTES,
  STALE_DRAFT_DAYS,
  staleDrafts,
  unknownStorageInfo,
  type StorageInfo,
} from './housekeeping';
export {
  MAX_DRAFT_PHOTO_BYTES,
  withPersistentPhotos,
  type PersistentPhotoDeps,
  type PhotoBlobStore,
} from './persistentPhotos';
export { AUTOSAVE_DELAY_MS, useDraftAutosave, type DraftAutosave } from './useDraftAutosave';

let deviceStore: DraftStore | null = null;

/** The device's drafts (SecureStore-backed; on the web, photos kept in IndexedDB), created on first use. */
export function deviceDraftStore(): DraftStore {
  deviceStore ??= withDevicePhotos(createDraftStore());
  return deviceStore;
}
