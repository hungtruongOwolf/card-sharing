import type { components } from '@card-sharing/api-client';

import { problemCode } from '../../data/problemCode';
import type { AuthClient } from '../auth';

export type OnboardingStep = components['schemas']['OnboardingStep'];
type AgeRequest = components['schemas']['AgeRequest'];
type ProfileRequest = components['schemas']['ProfileRequest'];
type HandleRequest = components['schemas']['HandleRequest'];

/** The part of `GET /v1/me` the onboarding flow reads. The region policy is deliberately not exposed here. */
export interface MeSummary {
  readonly step: OnboardingStep;
}

export type OnboardingErrorKind =
  /** 403 `AGE_GATE_REFUSED`: neutral refusal; the server removed the pending account. */
  | 'refused'
  /** 403 `AGE_ALREADY_SUBMITTED`: the date of birth is locked (AC-2.3). */
  | 'already_submitted'
  /** 400 `VALIDATION_FAILED` (bad format, profanity filter). */
  | 'invalid'
  /** 409 `HANDLE_TAKEN`. */
  | 'taken'
  /** 429 (`RATE_LIMITED`, `HANDLE_CHANGE_COOLDOWN`). */
  | 'rate_limited'
  /** Network, 5xx, unexpected status or body; retry later. */
  | 'unavailable';

export class OnboardingError extends Error {
  override name = 'OnboardingError';
  constructor(
    readonly kind: OnboardingErrorKind,
    readonly status?: number,
  ) {
    super(status === undefined ? `onboarding: ${kind}` : `onboarding: ${kind} (${status})`);
  }
}

export interface OnboardingApi {
  getMe(): Promise<MeSummary>;
  /** `POST /v1/me/age`. Resolves with the next step; refusal rejects with kind `refused`. */
  submitBirthDate(birthDate: string): Promise<OnboardingStep>;
  /** `PUT /v1/me/profile` (display name only; the avatar comes with the capture flow). */
  saveDisplayName(displayName: string): Promise<void>;
  /** `PUT /v1/me/handle` (409 `HANDLE_TAKEN`, 429 `HANDLE_CHANGE_COOLDOWN`). */
  claimHandle(handle: string): Promise<void>;
}

const STEPS: readonly OnboardingStep[] = ['age', 'profile', 'done'];

/** Reads `onboarding.step` from an untrusted body; anything unexpected is an error, never a guess. */
function stepFrom(body: unknown): OnboardingStep | null {
  if (typeof body !== 'object' || body === null || !('onboarding' in body)) return null;
  const onboarding = (body as { onboarding: unknown }).onboarding;
  if (typeof onboarding !== 'object' || onboarding === null || !('step' in onboarding)) return null;
  const step = (onboarding as { step: unknown }).step;
  return STEPS.find((s) => s === step) ?? null;
}

async function failure(response: Response): Promise<OnboardingError> {
  const { status } = response;
  const code = await problemCode(response);
  if (code === 'AGE_GATE_REFUSED') return new OnboardingError('refused', status);
  if (code === 'AGE_ALREADY_SUBMITTED') return new OnboardingError('already_submitted', status);
  if (code === 'HANDLE_TAKEN' || status === 409) return new OnboardingError('taken', status);
  if (status === 429) return new OnboardingError('rate_limited', status);
  if (status === 400) return new OnboardingError('invalid', status);
  return new OnboardingError('unavailable', status);
}

/**
 * Onboarding calls over the authorized `fetch` of the auth client (T-21), which owns tokens, refresh and
 * sign-out on a revoked session (those surface as `AuthError`, not `OnboardingError`).
 */
export function createOnboardingApi(fetcher: AuthClient['fetch']): OnboardingApi {
  const send = (path: string, method: string, body?: unknown) =>
    fetcher(path, {
      method,
      ...(body === undefined
        ? {}
        : { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }),
    });

  const readStep = async (response: Response): Promise<OnboardingStep> => {
    if (!response.ok) throw await failure(response);
    const step = stepFrom(await response.json().catch(() => null));
    if (!step) throw new OnboardingError('unavailable', response.status);
    return step;
  };

  const expectOk = async (response: Response): Promise<void> => {
    if (!response.ok) throw await failure(response);
  };

  return {
    getMe: async () => ({ step: await readStep(await send('/v1/me', 'GET')) }),
    submitBirthDate: async (birthDate) => {
      const body: AgeRequest = { birth_date: birthDate };
      return readStep(await send('/v1/me/age', 'POST', body));
    },
    saveDisplayName: async (displayName) => {
      const body: ProfileRequest = { display_name: displayName };
      await expectOk(await send('/v1/me/profile', 'PUT', body));
    },
    claimHandle: async (handle) => {
      const body: HandleRequest = { handle };
      await expectOk(await send('/v1/me/handle', 'PUT', body));
    },
  };
}
