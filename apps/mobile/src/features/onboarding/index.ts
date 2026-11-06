import type { AuthClient } from '../auth';
import { apiBaseUrl } from '../auth/config';
import type { CameraPermissions } from './cameraPermissions';
import { createExpoCameraPermissions } from './expoCameraPermissions';
import { createOnboardingApi, type OnboardingApi } from './onboardingApi';
import { createDevSignIn, type DevSignIn } from './devSignIn';
import { createAppSignInProviders, type SignInProvider } from './signInProviders';

export type { CameraPermissions, CameraPermissionStatus } from './cameraPermissions';
export { createDevSignIn, DevSignInError, type DevSignIn, type SeedUser } from './devSignIn';
export { createExpoCameraPermissions } from './expoCameraPermissions';
export { OnboardingProvider, useOnboarding } from './OnboardingProvider';
export type { OnboardingPhase } from './phase';
export { BirthDateScreen } from './screens/BirthDateScreen';
export { CameraPermissionScreen } from './screens/CameraPermissionScreen';
export { DisplayNameScreen } from './screens/DisplayNameScreen';
export { HandleScreen } from './screens/HandleScreen';
export { RefusedScreen } from './screens/RefusedScreen';
export { SignInScreen } from './screens/SignInScreen';
export { ErrorScreen, LoadingScreen } from './screens/StatusScreens';

export interface OnboardingDeps {
  api: OnboardingApi;
  signInProviders: readonly SignInProvider[];
  /** Development builds only (#306); null in release builds. */
  devSignIn: DevSignIn | null;
  camera: CameraPermissions;
}

/** Production wiring: the API over the app's auth client, the native sign-in sheets, the OS camera permission. */
export function createAppOnboardingDeps(authClient: AuthClient): OnboardingDeps {
  return {
    api: createOnboardingApi(authClient.fetch),
    signInProviders: createAppSignInProviders(),
    devSignIn: __DEV__ ? createDevSignIn({ baseUrl: apiBaseUrl, authClient }) : null,
    camera: createExpoCameraPermissions(),
  };
}
