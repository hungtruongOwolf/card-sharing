import type { AuthStatus } from '../auth';
import type { OnboardingStep } from './onboardingApi';

/**
 * Which screen the app shows. Each phase maps to exactly one route, guarded with `Stack.Protected` in
 * src/app/_layout.tsx, so a screen is reachable only while its phase is active: once the server moves the
 * account past `age`, the DOB screen cannot be opened again (AC-2.3).
 */
export type OnboardingPhase =
  | 'loading'
  | 'error'
  | 'signIn'
  | 'refused'
  | 'birthDate'
  | 'displayName'
  | 'handle'
  | 'camera'
  | 'home'
  /** The account is waiting out its deletion grace period (T-151/T-153): only "cancel" or "sign out". */
  | 'deletionPending';

export interface PhaseInput {
  readonly auth: AuthStatus;
  /** Server step from `GET /v1/me`; undefined while it loads. */
  readonly step: OnboardingStep | undefined;
  /** `account.status = deletion_pending` from `GET /v1/me`. */
  readonly deletionPending?: boolean;
  readonly meFailed: boolean;
  /** The age gate refused this device in this app session; cleared by "Back to start". */
  readonly refused: boolean;
  /** The display name was saved in this app session (the server has one `profile` step for name + handle). */
  readonly displayNameSaved: boolean;
  /** The camera explainer was handled in this app session (granted, skipped or library chosen). */
  readonly cameraHandled: boolean;
}

export function resolvePhase(input: PhaseInput): OnboardingPhase {
  // The refusal screen outlives the session it ended: the account is gone, the copy stays until dismissed.
  if (input.refused) return 'refused';
  if (input.auth === 'loading') return 'loading';
  if (input.auth === 'signedOut') return 'signIn';
  if (input.step === undefined) return input.meFailed ? 'error' : 'loading';
  if (input.deletionPending) return 'deletionPending';
  switch (input.step) {
    case 'age':
      return 'birthDate';
    case 'profile':
      return input.displayNameSaved ? 'handle' : 'displayName';
    case 'done':
      return input.cameraHandled ? 'home' : 'camera';
  }
}
