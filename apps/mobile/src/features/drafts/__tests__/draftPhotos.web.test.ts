import { DRAFT_PHOTO_DB, indexedDbPhotoStore } from '../draftPhotos.web';

/** The slice of IndexedDB the adapter uses: one database, one object store, async request callbacks. */
function fakeIndexedDb() {
  const data = new Map<string, unknown>();
  const stores = new Set<string>();
  const opened: string[] = [];
  const request = <T>(run: () => T) => {
    const req: { result?: T; error?: unknown; onsuccess?: () => void; onerror?: () => void } = {};
    setTimeout(() => {
      req.result = run();
      req.onsuccess?.();
    }, 0);
    return req;
  };
  const db = {
    objectStoreNames: { contains: (name: string) => stores.has(name) },
    createObjectStore: (name: string) => stores.add(name),
    transaction: (name: string) => {
      if (!stores.has(name)) throw new Error('NotFoundError');
      return {
        objectStore: () => ({
          get: (key: string) => request(() => data.get(key)),
          put: (value: unknown, key: string) => request(() => data.set(key, value)),
          delete: (key: string) => request(() => data.delete(key)),
        }),
      };
    },
  };
  const factory = {
    open: (name: string) => {
      opened.push(name);
      const req: {
        result?: typeof db;
        onupgradeneeded?: () => void;
        onsuccess?: () => void;
        onerror?: () => void;
      } = {};
      setTimeout(() => {
        req.result = db;
        if (!stores.size) req.onupgradeneeded?.();
        req.onsuccess?.();
      }, 0);
      return req;
    },
  };
  return { factory: factory as unknown as IDBFactory, data, opened };
}

describe('indexedDbPhotoStore', () => {
  it('stores, reads and deletes photo blobs by draft id in one database', async () => {
    const idb = fakeIndexedDb();
    const store = indexedDbPhotoStore(idb.factory);
    const blob = new Blob([new Uint8Array(4)], { type: 'image/png' });
    expect(await store.get('d1')).toBeNull();
    await store.put('d1', blob);
    expect(await store.get('d1')).toBe(blob);
    await store.delete('d1');
    expect(await store.get('d1')).toBeNull();
    expect(idb.opened).toEqual([DRAFT_PHOTO_DB]);
  });

  it('ignores anything stored that is not a Blob', async () => {
    const idb = fakeIndexedDb();
    const store = indexedDbPhotoStore(idb.factory);
    await store.put('d1', new Blob([]));
    idb.data.set('d1', 'not a blob');
    expect(await store.get('d1')).toBeNull();
  });

  it('fails (and the caller carries on) when IndexedDB is missing', async () => {
    await expect(indexedDbPhotoStore(undefined).get('d1')).rejects.toThrow();
  });
});
