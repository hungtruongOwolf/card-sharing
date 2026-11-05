import type { IdTokenProvider } from '../auth';

/** What a native sign-in sheet hands back: the provider ID token and the raw nonce the app generated for it. */
export interface IdTokenCredential {
  readonly idToken: string;
  /** Raw nonce (16–256 visible ASCII); the provider was given its SHA-256 (api-mvp `POST /v1/auth/apple`). */
  readonly nonce: string;
  /** Apple only: one-time authorization code from the same sheet. */
  readonly authorizationCode?: string;
}

/**
 * One sign-in button's native side. The screen calls `getCredential()` and passes the result to
 * `AuthClient.signInWithIdToken`; it never sees how the token was obtained.
 */
export interface SignInProvider {
  readonly id: IdTokenProvider;
  /** False hides nothing: the button stays visible and explains that the option is not available yet. */
  isAvailable(): boolean;
  /** Opens the provider sheet. Resolves null when the user cancels. */
  getCredential(): Promise<IdTokenCredential | null>;
}

export class SignInUnavailableError extends Error {
  override name = 'SignInUnavailableError';
}

/**
 * Placeholder until the native sheets exist. Apple (T-19) and Google (T-20) need provider accounts and a
 * development build (expo-apple-authentication, Google Sign-In, expo-crypto for the nonce hash); the server
 * side is tested against a mocked JWKS meanwhile. There is no test-login route on the API, so release and
 * development builds alike can only exercise the flow past this screen in tests (stubbed at this seam).
 */
function notYetAvailable(id: IdTokenProvider): SignInProvider {
  return {
    id,
    isAvailable: () => false,
    getCredential: () => Promise.reject(new SignInUnavailableError(id)),
  };
}

/** Buttons in display order. */
export function createAppSignInProviders(): readonly SignInProvider[] {
  return [notYetAvailable('apple'), notYetAvailable('google')];
}
