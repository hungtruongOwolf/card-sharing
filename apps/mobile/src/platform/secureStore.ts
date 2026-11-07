/**
 * Secure key-value storage used by the app's stores (session, drafts, feed and board copies, outbox, prefs, widget
 * token). Native: expo-secure-store (Keychain on iOS, Keystore-encrypted on Android). The web target resolves
 * `secureStore.web.ts` instead, because expo-secure-store has no web implementation (ADR-0009).
 *
 * Only the subset the app uses is exported; the web file exports the same names with the same types.
 */
export {
  AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY,
  deleteItemAsync,
  getItemAsync,
  setItemAsync,
  WHEN_UNLOCKED_THIS_DEVICE_ONLY,
  type SecureStoreOptions,
} from 'expo-secure-store';
