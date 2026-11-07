import Constants from 'expo-constants';

import { createAuthClient, type AuthClient } from './authClient';
import { apiBaseUrl } from './config';
import { createSecureTokenStore } from './tokenStore';

export { AuthError, createAuthClient } from './authClient';
export type {
  AuthClient,
  AuthErrorKind,
  AuthState,
  AuthStatus,
  IdTokenProvider,
  SignInOptions,
  SignOutReason,
} from './authClient';
export { AuthProvider, useAuthClient, useAuthState } from './AuthProvider';

/** The app's auth client: secure-store session, `EXPO_PUBLIC_API_URL`, app version from app config. */
export function createAppAuthClient(): AuthClient {
  return createAuthClient({
    baseUrl: apiBaseUrl,
    store: createSecureTokenStore(),
    appVersion: Constants.expoConfig?.version,
    streamFetch: streamingFetch,
  });
}

/**
 * `expo/fetch` streams response bodies on iOS, Android and the web (the built-in React Native fetch does not). It is
 * part of the `expo` package, so it works in Expo Go without a new native module; loaded on first use only.
 */
const streamingFetch: typeof fetch = async (input, init) => {
  const { fetch: expoFetch } = await import('expo/fetch');
  const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
  return (await expoFetch(url, init as Parameters<typeof expoFetch>[1])) as unknown as Response;
};
