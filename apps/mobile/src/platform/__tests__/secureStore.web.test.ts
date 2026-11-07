import * as web from '../secureStore.web';

function fakeStorage() {
  const items = new Map<string, string>();
  return {
    items,
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => void items.set(key, value),
    removeItem: (key: string) => void items.delete(key),
  };
}

afterEach(() => web.setWebBackendForTests(null));

it('exports the same names as the native module', () => {
  // Jest resolves the native file (the import resolver used by lint would pick the .web file).
  const native: object = jest.requireActual('../secureStore');
  expect(Object.keys(web)).toEqual(expect.arrayContaining(Object.keys(native)));
});

it('stores, reads and deletes values under the app prefix', async () => {
  const storage = fakeStorage();
  web.setWebBackendForTests(storage);
  await web.setItemAsync('cs.auth.session.v1', 'value', {
    keychainAccessible: web.AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY,
  });
  expect(storage.items.get('cs.secure.cs.auth.session.v1')).toBe('value');
  await expect(web.getItemAsync('cs.auth.session.v1')).resolves.toBe('value');
  await web.deleteItemAsync('cs.auth.session.v1');
  await expect(web.getItemAsync('cs.auth.session.v1')).resolves.toBeNull();
});

it('rejects keys the native store would reject', async () => {
  web.setWebBackendForTests(fakeStorage());
  await expect(web.setItemAsync('bad key/1', 'x')).rejects.toThrow('Invalid secure store key');
});

it('falls back to memory when browser storage is unavailable', async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    get() {
      throw new Error('SecurityError');
    },
  });
  try {
    await web.setItemAsync('cs.drafts.index.v1', '[]');
    await expect(web.getItemAsync('cs.drafts.index.v1')).resolves.toBe('[]');
  } finally {
    if (original) Object.defineProperty(globalThis, 'localStorage', original);
    else delete (globalThis as { localStorage?: unknown }).localStorage;
  }
});
