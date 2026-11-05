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
  });
}
