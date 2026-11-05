import { useQuery, useQueryClient } from '@tanstack/react-query';
import { createContext, useContext, useMemo, useState, type ReactNode } from 'react';

import { useAuthClient, useAuthState } from '../auth';
import type { CameraPermissions } from './cameraPermissions';
import { OnboardingError, type OnboardingApi, type OnboardingStep } from './onboardingApi';
import { resolvePhase, type OnboardingPhase } from './phase';
import type { SignInProvider } from './signInProviders';

export const ME_QUERY_KEY = ['me'] as const;

export interface OnboardingContextValue {
  readonly phase: OnboardingPhase;
  readonly signInProviders: readonly SignInProvider[];
  readonly camera: CameraPermissions;
  /** Re-reads `/v1/me` after a failure. */
  retry(): void;
  /**
   * Submits the date of birth once. Refusal ends the session on the device and shows the refusal screen;
   * an already-locked DOB just moves on to whatever step the server reports. Other failures reject.
   */
  submitBirthDate(birthDate: string): Promise<void>;
  saveDisplayName(displayName: string): Promise<void>;
  claimHandle(handle: string): Promise<void>;
  /** The camera explainer is done (granted, skipped or a library photo chosen). */
  finishCamera(): void;
  /** "Back to start" on the refusal screen. */
  acknowledgeRefusal(): void;
}

const OnboardingContext = createContext<OnboardingContextValue | null>(null);

interface Props {
  api: OnboardingApi;
  signInProviders: readonly SignInProvider[];
  camera: CameraPermissions;
  children: ReactNode;
}

/**
 * Drives the onboarding stack from the server's `onboarding.step` (GET /v1/me). Local flags cover only what
 * the server does not model: the refusal screen after the account is gone, the name → handle order inside
 * the single `profile` step, and the camera explainer. Must sit inside `AuthProvider`.
 */
export function OnboardingProvider({ api, signInProviders, camera, children }: Props) {
  const auth = useAuthState();
  const authClient = useAuthClient();
  const queryClient = useQueryClient();
  const [refused, setRefused] = useState(false);
  const [displayNameSaved, setDisplayNameSaved] = useState(false);
  const [cameraHandled, setCameraHandled] = useState(false);

  const signedIn = auth.status === 'signedIn';
  const me = useQuery({
    queryKey: ME_QUERY_KEY,
    queryFn: () => api.getMe(),
    enabled: signedIn && !refused,
  });
  const refetch = me.refetch;

  // Session-local progress belongs to the account that made it: forget it whenever the device signs out.
  // Adjusted during render (React's "reset state when a value changes" pattern), not in an effect.
  const [lastAuthStatus, setLastAuthStatus] = useState(auth.status);
  if (auth.status !== lastAuthStatus) {
    setLastAuthStatus(auth.status);
    if (auth.status === 'signedOut') {
      setDisplayNameSaved(false);
      setCameraHandled(false);
    }
  }

  const phase = resolvePhase({
    auth: auth.status,
    step: signedIn ? me.data?.step : undefined,
    meFailed: me.isError,
    refused,
    displayNameSaved,
    cameraHandled,
  });

  const value = useMemo<OnboardingContextValue>(() => {
    const setStep = (step: OnboardingStep) =>
      queryClient.setQueryData(ME_QUERY_KEY, { step } satisfies { step: OnboardingStep });
    const refetchMe = () => queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY });

    return {
      phase,
      signInProviders,
      camera,
      retry: () => void refetch(),
      async submitBirthDate(birthDate) {
        try {
          setStep(await api.submitBirthDate(birthDate));
        } catch (err) {
          if (err instanceof OnboardingError && err.kind === 'refused') {
            setRefused(true);
            // The server already deleted the pending account; clear this device's session too.
            await authClient.signOut();
            return;
          }
          if (err instanceof OnboardingError && err.kind === 'already_submitted') {
            await refetchMe();
            return;
          }
          throw err;
        }
      },
      async saveDisplayName(displayName) {
        await api.saveDisplayName(displayName);
        setDisplayNameSaved(true);
      },
      async claimHandle(handle) {
        await api.claimHandle(handle);
        await refetchMe();
        // Still `profile` after a claim means the name was not stored (e.g. app restarted): ask for it again.
        const step = queryClient.getQueryData<{ step: OnboardingStep }>(ME_QUERY_KEY)?.step;
        if (step === 'profile') setDisplayNameSaved(false);
      },
      finishCamera: () => setCameraHandled(true),
      acknowledgeRefusal() {
        setRefused(false);
        setDisplayNameSaved(false);
        setCameraHandled(false);
      },
    };
  }, [phase, signInProviders, camera, refetch, api, authClient, queryClient]);

  return <OnboardingContext.Provider value={value}>{children}</OnboardingContext.Provider>;
}

export function useOnboarding(): OnboardingContextValue {
  const value = useContext(OnboardingContext);
  if (!value) throw new Error('useOnboarding must be used inside OnboardingProvider');
  return value;
}
