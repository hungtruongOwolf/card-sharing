/**
 * In-memory replacement for expo-secure-store (native-only) for Jest:
 * `jest.mock('expo-secure-store', () => require('../../test/secureStoreMock').secureStoreMock)`.
 * Records the options of every call so tests can assert the keychain accessibility class.
 */
const items = new Map<string, string>();
const calls: { op: 'get' | 'set' | 'delete'; key: string; options: unknown }[] = [];

export const secureStoreMock = {
  AFTER_FIRST_UNLOCK: 0,
  AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY: 1,
  ALWAYS: 2,
  WHEN_PASSCODE_SET_THIS_DEVICE_ONLY: 3,
  ALWAYS_THIS_DEVICE_ONLY: 4,
  WHEN_UNLOCKED: 5,
  WHEN_UNLOCKED_THIS_DEVICE_ONLY: 6,
  getItemAsync: jest.fn(async (key: string, options?: unknown) => {
    calls.push({ op: 'get', key, options });
    return items.get(key) ?? null;
  }),
  setItemAsync: jest.fn(async (key: string, value: string, options?: unknown) => {
    calls.push({ op: 'set', key, options });
    items.set(key, value);
  }),
  deleteItemAsync: jest.fn(async (key: string, options?: unknown) => {
    calls.push({ op: 'delete', key, options });
    items.delete(key);
  }),
  __items: items,
  __calls: calls,
  __reset() {
    items.clear();
    calls.length = 0;
  },
};
