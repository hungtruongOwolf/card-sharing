import type { AuthClient } from '../auth';
import type { CameraPermissions } from './cameraPermissions';
import { createExpoCameraPermissions } from './expoCameraPermissions';
import { createOnboardingApi, type OnboardingApi } from './onboardingApi';
import { createAppSignInProviders, type SignInProvider } from './signInProviders';

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
  camera: CameraPermissions;
}

/** Production wiring: the API over the app's auth client, the native sign-in sheets, the OS camera permission. */
export function createAppOnboardingDeps(authClient: AuthClient): OnboardingDeps {
  return {
    api: createOnboardingApi(authClient.fetch),
    signInProviders: createAppSignInProviders(),
    camera: createExpoCameraPermissions(),
  };
}
