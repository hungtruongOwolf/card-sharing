import { useQuery, useQueryClient } from '@tanstack/react-query';
import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';

import { isServerBusy } from '../../data/serverBusy';
import { useAuthClient, useAuthState } from '../auth';
import type { CameraPermissions } from './cameraPermissions';
import {
  OnboardingError,
  type MeSummary,
  type OnboardingApi,
  type OnboardingStep,
} from './onboardingApi';
import { resolvePhase, type OnboardingPhase } from './phase';
import type { DevSignIn } from './devSignIn';
import type { SignInProvider } from './signInProviders';

export const ME_QUERY_KEY = ['me'] as const;

export interface OnboardingContextValue {
  readonly phase: OnboardingPhase;
  readonly signInProviders: readonly SignInProvider[];
  /** Development builds only (#306): sign in as a seed user of a local server; null otherwise. */
  readonly devSignIn: DevSignIn | null;
  /** `/v1/me` `policy.public_links.allowed` (#321); null until known or when the server did not say. */
  readonly publicLinksAllowed: boolean | null;
  readonly camera: CameraPermissions;
  /** `/v1/me` failed because the server is busy (503 `SERVER_BUSY`), not unreachable. */
  readonly serverBusy: boolean;
  /** Re-reads `/v1/me` after a failure. */
  retry(): void;
  /** Re-reads `/v1/me`, e.g. after a pending deletion was cancelled. */
  refreshAccount(): Promise<void>;
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
  devSignIn?: DevSignIn | null;
  camera: CameraPermissions;
  children: ReactNode;
}

/**
 * Drives the onboarding stack from the server's `onboarding.step` (GET /v1/me). Local flags cover only what
 * the server does not model: the refusal screen after the account is gone, the name → handle order inside
 * the single `profile` step, and the camera explainer. Must sit inside `AuthProvider`.
 */
export function OnboardingProvider({
  api,
  signInProviders,
  devSignIn = null,
  camera,
  children,
}: Props) {
  const auth = useAuthState();
  const authClient = useAuthClient();
  const queryClient = useQueryClient();
  const [refused, setRefused] = useState(false);
  const [displayNameSaved, setDisplayNameSaved] = useState(false);
  const [cameraHandled, setCameraHandled] = useState(false);
  const [cameraGranted, setCameraGranted] = useState<boolean | undefined>(undefined);

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
      setCameraGranted(undefined);
    }
  }

  // An onboarded account skips the camera explainer when the permission is already granted: read it before
  // choosing the phase, so a launch does not route through the explainer (and lose the URL it was opened at).
  const cameraCheckNeeded =
    signedIn && me.data?.step === 'done' && !cameraHandled && cameraGranted === undefined;
  useEffect(() => {
    if (!cameraCheckNeeded) return;
    let cancelled = false;
    void camera
      .get()
      .then(
        (status) => status === 'granted',
        () => false,
      )
      .then((granted) => {
        if (!cancelled) setCameraGranted(granted);
      });
    return () => {
      cancelled = true;
    };
  }, [cameraCheckNeeded, camera]);

  const publicLinksAllowed = (signedIn ? me.data?.publicLinksAllowed : null) ?? null;
  const phase = resolvePhase({
    auth: auth.status,
    step: signedIn ? me.data?.step : undefined,
    deletionPending: signedIn && me.data?.deletionPending === true,
    meFailed: me.isError,
    refused,
    displayNameSaved,
    cameraHandled,
    cameraGranted,
  });

  const serverBusy = me.isError && isServerBusy(me.error);

  const value = useMemo<OnboardingContextValue>(() => {
    const setStep = (step: OnboardingStep) =>
      queryClient.setQueryData(ME_QUERY_KEY, {
        step,
        deletionPending: false,
        publicLinksAllowed: null,
      } satisfies MeSummary);
    const refetchMe = () => queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY });

    return {
      phase,
      signInProviders,
      devSignIn,
      publicLinksAllowed,
      camera,
      serverBusy,
      retry: () => void refetch(),
      refreshAccount: refetchMe,
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
        const step = queryClient.getQueryData<MeSummary>(ME_QUERY_KEY)?.step;
        if (step === 'profile') setDisplayNameSaved(false);
      },
      finishCamera: () => setCameraHandled(true),
      acknowledgeRefusal() {
        setRefused(false);
        setDisplayNameSaved(false);
        setCameraHandled(false);
        setCameraGranted(undefined);
      },
    };
  }, [
    phase,
    signInProviders,
    devSignIn,
    publicLinksAllowed,
    camera,
    serverBusy,
    refetch,
    api,
    authClient,
    queryClient,
  ]);

  return <OnboardingContext.Provider value={value}>{children}</OnboardingContext.Provider>;
}

export function useOnboarding(): OnboardingContextValue {
  const value = useContext(OnboardingContext);
  if (!value) throw new Error('useOnboarding must be used inside OnboardingProvider');
  return value;
}
