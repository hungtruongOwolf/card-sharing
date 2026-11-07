import type { components } from '@card-sharing/api-client';

import { problemCode } from '../../data/problemCode';
import { SERVER_BUSY } from '../../data/serverBusy';
import type { AuthClient } from '../auth';

export type OnboardingStep = components['schemas']['OnboardingStep'];
type AgeRequest = components['schemas']['AgeRequest'];
type ProfileRequest = components['schemas']['ProfileRequest'];
type HandleRequest = components['schemas']['HandleRequest'];

/**
 * The part of `GET /v1/me` the app reads. The region policy itself (thresholds, ages) is deliberately not exposed
 * here, only the yes/no hints the server resolved for this account.
 */
export interface MeSummary {
  readonly step: OnboardingStep;
  /**
   * `account.status = deletion_pending` (T-151): the account asked to be deleted and is in its grace period.
   * Its session may only read and cancel the deletion, so the app shows nothing else.
   */
  readonly deletionPending: boolean;
  /**
   * `policy.public_links.allowed` (#321): whether this account may create public board links (AC-24.6). A client
   * hint only; null when the server did not say, and the create route stays the authority.
   */
  readonly publicLinksAllowed: boolean | null;
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
  /** 503 `SERVER_BUSY`: the server is overloaded, not down; retry after `Retry-After`. */
  | 'busy'
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

function accountStatusFrom(body: unknown): unknown {
  if (typeof body !== 'object' || body === null || !('account' in body)) return undefined;
  const account = (body as { account: unknown }).account;
  if (typeof account !== 'object' || account === null || !('status' in account)) return undefined;
  return (account as { status: unknown }).status;
}

function publicLinksAllowedFrom(body: unknown): boolean | null {
  const policy =
    typeof body === 'object' && body !== null ? (body as { policy?: unknown }).policy : null;
  const links =
    typeof policy === 'object' && policy !== null
      ? (policy as { public_links?: unknown }).public_links
      : null;
  const allowed =
    typeof links === 'object' && links !== null ? (links as { allowed?: unknown }).allowed : null;
  return typeof allowed === 'boolean' ? allowed : null;
}

async function failure(response: Response): Promise<OnboardingError> {
  const { status } = response;
  const code = await problemCode(response);
  if (code === 'AGE_GATE_REFUSED') return new OnboardingError('refused', status);
  if (code === 'AGE_ALREADY_SUBMITTED') return new OnboardingError('already_submitted', status);
  if (code === 'HANDLE_TAKEN' || status === 409) return new OnboardingError('taken', status);
  if (status === 429) return new OnboardingError('rate_limited', status);
  if (status === 400) return new OnboardingError('invalid', status);
  if (status === 503 && code === SERVER_BUSY) return new OnboardingError('busy', status);
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

  const readMe = async (response: Response): Promise<MeSummary> => {
    if (!response.ok) throw await failure(response);
    const body: unknown = await response.json().catch(() => null);
    const step = stepFrom(body);
    if (!step) throw new OnboardingError('unavailable', response.status);
    return {
      step,
      deletionPending: accountStatusFrom(body) === 'deletion_pending',
      publicLinksAllowed: publicLinksAllowedFrom(body),
    };
  };

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
    getMe: async () => readMe(await send('/v1/me', 'GET')),
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
