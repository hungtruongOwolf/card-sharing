import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { View } from 'react-native';

import { AuthProvider, useAuthClient } from '../features/auth/AuthProvider';
import { createAuthClient, type AuthClient } from '../features/auth/authClient';
import { sessionFromTokenPair, type Session } from '../features/auth/session';
import type { TokenStore } from '../features/auth/tokenStore';
import type {
  CameraPermissions,
  CameraPermissionStatus,
} from '../features/onboarding/cameraPermissions';
import { createOnboardingApi } from '../features/onboarding/onboardingApi';
import { OnboardingProvider, useOnboarding } from '../features/onboarding/OnboardingProvider';
import { BirthDateScreen } from '../features/onboarding/screens/BirthDateScreen';
import { CameraPermissionScreen } from '../features/onboarding/screens/CameraPermissionScreen';
import { DisplayNameScreen } from '../features/onboarding/screens/DisplayNameScreen';
import { HandleScreen } from '../features/onboarding/screens/HandleScreen';
import { RefusedScreen } from '../features/onboarding/screens/RefusedScreen';
import { SignInScreen } from '../features/onboarding/screens/SignInScreen';
import { ErrorScreen, LoadingScreen } from '../features/onboarding/screens/StatusScreens';
import type { IdTokenCredential, SignInProvider } from '../features/onboarding/signInProviders';
import { DeletionPendingScreen } from '../features/privacy/DeletionPendingScreen';
import { createPrivacyApi } from '../features/privacy/privacyApi';
import type { FakeOnboardingServer } from './fakeOnboardingServer';
import { renderThemed } from './render';

export const TODAY = new Date(2026, 10, 5);
export const VALID_CREDENTIAL: IdTokenCredential = {
  idToken: 'valid-id-token',
  nonce: 'n0nce-0123456789abcdef',
};

/** In-memory token store (the secure store has its own tests in T-21). */
export function memoryStore(
  initial: Session | null = null,
): TokenStore & { current: Session | null } {
  const store = {
    current: initial,
    load: async () => store.current,
    save: async (session: Session) => {
      store.current = session;
    },
    clear: async () => {
      store.current = null;
    },
  };
  return store;
}

export interface FakeCamera extends CameraPermissions {
  status: CameraPermissionStatus;
  /** What the OS prompt answers on `request()`. */
  answer: CameraPermissionStatus;
  /** Whether the library picker returns a photo. */
  picks: boolean;
  readonly openSettings: jest.Mock<Promise<void>, []>;
  readonly pickFromLibrary: jest.Mock<Promise<boolean>, []>;
  readonly request: jest.Mock<Promise<CameraPermissionStatus>, []>;
}

export function fakeCamera(status: CameraPermissionStatus = 'undetermined'): FakeCamera {
  const camera: FakeCamera = {
    status,
    answer: 'granted',
    picks: true,
    get: async () => camera.status,
    request: jest.fn(async () => {
      camera.status = camera.answer;
      return camera.status;
    }),
    openSettings: jest.fn(async () => undefined),
    pickFromLibrary: jest.fn(async () => camera.picks),
  };
  return camera;
}

export function fakeProvider(
  id: SignInProvider['id'],
  credential: IdTokenCredential | null | 'unavailable' = VALID_CREDENTIAL,
): SignInProvider & { getCredential: jest.Mock } {
  return {
    id,
    isAvailable: () => credential !== 'unavailable',
    getCredential: jest.fn(async () => (credential === 'unavailable' ? null : credential)),
  };
}

/** Same phase → screen mapping as the guarded stack in src/app/_layout.tsx, without the native navigator. */
function PhaseSwitch() {
  const { phase, refreshAccount } = useOnboarding();
  const authClient = useAuthClient();
  switch (phase) {
    case 'loading':
      return <LoadingScreen />;
    case 'error':
      return <ErrorScreen />;
    case 'signIn':
      return <SignInScreen />;
    case 'refused':
      return <RefusedScreen />;
    case 'birthDate':
      return <BirthDateScreen today={() => TODAY} locale="en-US" />;
    case 'displayName':
      return <DisplayNameScreen />;
    case 'handle':
      return <HandleScreen />;
    case 'camera':
      return <CameraPermissionScreen />;
    case 'home':
      return <View testID="home" />;
    case 'deletionPending':
      return (
        <DeletionPendingScreen
          api={createPrivacyApi(authClient)}
          onAccountChanged={refreshAccount}
          signOut={authClient.signOut}
        />
      );
  }
}

interface Options {
  signedIn?: boolean;
  camera?: FakeCamera;
  providers?: readonly SignInProvider[];
}

export async function renderOnboarding(server: FakeOnboardingServer, options: Options = {}) {
  const { signedIn = false, camera = fakeCamera(), providers = [fakeProvider('apple')] } = options;
  const store = memoryStore(signedIn ? sessionFromTokenPair(server.auth.issue()) : null);
  const authClient: AuthClient = createAuthClient({
    baseUrl: () => server.baseUrl,
    store,
    fetch: server.fetch,
    now: server.now,
    logoutTimeoutMs: 50,
  });
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: 0 } },
  });
  const result = await renderThemed(
    <QueryClientProvider client={queryClient}>
      <AuthProvider client={authClient}>
        <OnboardingProvider
          api={createOnboardingApi(authClient.fetch)}
          signInProviders={providers}
          camera={camera}
        >
          <PhaseSwitch />
        </OnboardingProvider>
      </AuthProvider>
    </QueryClientProvider>,
  );
  return { ...result, authClient, store, camera, queryClient };
}
