import type { components } from '@card-sharing/api-client';

import { problemCode } from '../../data/problemCode';
import type { AuthClient } from '../auth';
import { mediaUrl } from '../sharing/cardsApi';

type Schemas = components['schemas'];

/** One curated template (validated `AiTemplateView`); the prompt stays on the server. */
export interface AiTemplate {
  readonly id: string;
  readonly version: number;
  /** Server-curated title (config/ai_effects.yaml), bounded. */
  readonly title: string;
  /** Credits one run costs. */
  readonly cost: number;
}

/** Validated `AiEffectsOverview` (`GET /v1/ai-effects`). */
export interface AiOverview {
  /** False = hide AI effects: no provider configured, or the region / age policy does not allow them. */
  readonly available: boolean;
  readonly consent: { readonly requiredVersion: number; readonly granted: boolean };
  readonly credits: number;
  readonly templates: readonly AiTemplate[];
  /** Region policy `ai_label.visible_badge`: show the visible "AI" badge on generated images. */
  readonly visibleBadge: boolean;
}

export type AiJobStatus = 'queued' | 'succeeded' | 'refused' | 'failed';

/** Validated `AiJobView` (owner only). */
export interface AiJob {
  readonly id: string;
  readonly templateId: string;
  readonly templateVersion: number;
  readonly status: AiJobStatus;
  /** Why it was refused or failed (`input_moderation`, `provider_failed`...), else null. */
  readonly outcome: string | null;
  readonly cost: number;
  readonly refunded: boolean;
  /** The generated `card_render` upload; null until succeeded. */
  readonly outputMediaId: string | null;
  /**
   * A short-lived signed URL of the output for the owner's preview, when the server sends one. Not in the contract
   * yet (the job view has no URL); until it is, the editor shows the template art instead of the result.
   */
  readonly outputUrl: string | null;
}

/** Media states of `GET /v1/media/{id}`. */
export type MediaState =
  'uploaded' | 'scanning' | 'clean' | 'held' | 'blocked' | 'failed' | 'deleted' | 'unknown';

export type AiEffectsErrorKind =
  /** 403 `CONSENT_REQUIRED`: no consent to the current text (never agreed, withdrawn, or the text changed). */
  | 'consent_required'
  /** 402 `INSUFFICIENT_CREDITS`. */
  | 'insufficient_credits'
  /** 403 `POLICY_DISALLOWED`: region or age policy; hide AI effects. */
  | 'policy_disallowed'
  /** Other 403 (`ONBOARDING_INCOMPLETE`, `ACCOUNT_SUSPENDED`). */
  | 'not_allowed'
  /** 404: the input or job is not mine (or gone). */
  | 'not_found'
  /** 400 `VALIDATION_FAILED` (a template switched off or bumped, an old consent version). */
  | 'invalid'
  /** The uploaded card did not pass the content checks (held, blocked, failed). */
  | 'input_rejected'
  /** 429. */
  | 'rate_limited'
  /** Network, 5xx (503 = no provider configured), 409 in progress, unexpected status or body. */
  | 'unavailable';

export class AiEffectsError extends Error {
  override name = 'AiEffectsError';
  constructor(
    readonly kind: AiEffectsErrorKind,
    readonly status?: number,
  ) {
    super(status === undefined ? `ai effects: ${kind}` : `ai effects: ${kind} (${status})`);
  }
}

/** Tier 2 cloud AI effects (genai-effects.md, api-mvp "AI effects"). Credits, consent and policy are the server's. */
export interface AiEffectsApi {
  /** `GET /v1/ai-effects`. */
  overview(): Promise<AiOverview>;
  /** `PUT /v1/ai-effects/consent {version}`: the version of the text the app showed. */
  grantConsent(version: number): Promise<void>;
  /** `POST /v1/ai-effects/jobs` with `Idempotency-Key` = `clientJobId`; a replay returns the same job, not charged. */
  createJob(job: {
    clientJobId: string;
    template: Pick<AiTemplate, 'id' | 'version'>;
    inputMediaId: string;
  }): Promise<AiJob>;
  /** `GET /v1/ai-effects/jobs/{id}`. */
  getJob(id: string): Promise<AiJob>;
  /** `GET /v1/media/{id}`: the state of one of my uploads. */
  mediaState(id: string): Promise<MediaState>;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
/** Template ids as config/ai_effects.yaml names them (`ai.watercolor`). */
const TEMPLATE_ID = /^[a-z0-9][a-z0-9._-]{0,63}$/;
const MAX_TITLE_LENGTH = 60;
const MAX_TEMPLATES = 50;
const MAX_OUTCOME_LENGTH = 64;
const JOB_STATUSES: readonly AiJobStatus[] = ['queued', 'succeeded', 'refused', 'failed'];
const MEDIA_STATES: readonly MediaState[] = [
  'uploaded',
  'scanning',
  'clean',
  'held',
  'blocked',
  'failed',
  'deleted',
];

export const isTemplateId = (value: string): boolean => TEMPLATE_ID.test(value);

const record = (value: unknown): Record<string, unknown> | null =>
  typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const count = (value: unknown): number | null =>
  Number.isSafeInteger(value) && (value as number) >= 0 ? (value as number) : null;

function parseTemplate(value: unknown): AiTemplate | null {
  const v = record(value);
  if (!v || typeof v.id !== 'string' || !TEMPLATE_ID.test(v.id)) return null;
  const version = count(v.version);
  const cost = count(v.cost);
  const title = typeof v.title === 'string' ? v.title.trim() : '';
  if (version === null || cost === null || title.length === 0 || title.length > MAX_TITLE_LENGTH) {
    return null;
  }
  return { id: v.id, version, title, cost };
}

/** Validates `AiEffectsOverview`; malformed templates are dropped, a malformed body is null. */
export function parseOverview(body: unknown): AiOverview | null {
  const v = record(body);
  const consent = record(v?.consent);
  const label = record(v?.ai_label);
  if (!v || typeof v.available !== 'boolean' || !consent || !Array.isArray(v.templates)) {
    return null;
  }
  const requiredVersion = count(consent.required_version);
  const credits = count(v.credits);
  if (requiredVersion === null || typeof consent.granted !== 'boolean' || credits === null) {
    return null;
  }
  const templates = v.available
    ? v.templates
        .slice(0, MAX_TEMPLATES)
        .map(parseTemplate)
        .filter((template): template is AiTemplate => template !== null)
    : [];
  return {
    available: v.available,
    consent: { requiredVersion, granted: consent.granted },
    credits,
    templates,
    // Fail safe: an unreadable label rule shows the badge.
    visibleBadge: label?.visible_badge !== false,
  };
}

/** Validates an `AiJobEnvelope`. */
export function parseJob(body: unknown): AiJob | null {
  const v = record(record(body)?.job);
  if (!v || typeof v.id !== 'string' || !UUID.test(v.id)) return null;
  if (typeof v.template_id !== 'string' || !TEMPLATE_ID.test(v.template_id)) return null;
  const status = JOB_STATUSES.find((known) => known === v.status);
  const templateVersion = count(v.template_version);
  const cost = count(v.cost);
  if (!status || templateVersion === null || cost === null || typeof v.refunded !== 'boolean') {
    return null;
  }
  const outputMediaId =
    typeof v.output_media_id === 'string' && UUID.test(v.output_media_id)
      ? v.output_media_id
      : null;
  // A success without an output is not usable: treat the body as malformed.
  if (status === 'succeeded' && !outputMediaId) return null;
  const outcome =
    typeof v.outcome === 'string' && v.outcome.length <= MAX_OUTCOME_LENGTH ? v.outcome : null;
  return {
    id: v.id,
    templateId: v.template_id,
    templateVersion,
    status,
    outcome,
    cost,
    refunded: v.refunded,
    outputMediaId,
    outputUrl: mediaUrl(v.output_url),
  };
}

export function parseMediaState(body: unknown): MediaState | null {
  const v = record(record(body)?.media);
  if (!v || typeof v.state !== 'string') return null;
  return MEDIA_STATES.find((known) => known === v.state) ?? 'unknown';
}

async function failure(response: Response): Promise<AiEffectsError> {
  const { status } = response;
  switch (await problemCode(response)) {
    case 'CONSENT_REQUIRED':
      return new AiEffectsError('consent_required', status);
    case 'INSUFFICIENT_CREDITS':
      return new AiEffectsError('insufficient_credits', status);
    case 'POLICY_DISALLOWED':
      return new AiEffectsError('policy_disallowed', status);
  }
  if (status === 402) return new AiEffectsError('insufficient_credits', status);
  if (status === 400) return new AiEffectsError('invalid', status);
  if (status === 403) return new AiEffectsError('not_allowed', status);
  if (status === 404) return new AiEffectsError('not_found', status);
  if (status === 429) return new AiEffectsError('rate_limited', status);
  return new AiEffectsError('unavailable', status);
}

/**
 * AI effects calls over the authorized `fetch` of the auth client. A network failure or a revoked session surfaces
 * as `AuthError` from the fetcher; everything else is an {@link AiEffectsError}. Ids reach a URL path only after validation.
 */
export function createAiEffectsApi(fetcher: AuthClient['fetch']): AiEffectsApi {
  const send = async (path: string, init: RequestInit): Promise<Response> => {
    const response = await fetcher(path, init);
    if (!response.ok) throw await failure(response);
    return response;
  };
  const read = async <T>(
    parse: (body: unknown) => T | null,
    path: string,
    init: RequestInit,
  ): Promise<T> => {
    const response = await send(path, init);
    const parsed = parse(await response.json().catch(() => undefined));
    if (parsed === null) throw new AiEffectsError('unavailable', response.status);
    return parsed;
  };
  const checkedId = (id: string): string => {
    if (!UUID.test(id)) throw new AiEffectsError('not_found');
    return encodeURIComponent(id);
  };

  return {
    overview: () => read(parseOverview, '/v1/ai-effects', { method: 'GET' }),
    async grantConsent(version) {
      if (!Number.isSafeInteger(version) || version < 0) throw new AiEffectsError('invalid');
      const body: Schemas['AiConsentRequest'] = { version };
      await send('/v1/ai-effects/consent', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
    },
    createJob({ clientJobId, template, inputMediaId }) {
      if (![clientJobId, inputMediaId].every((id) => UUID.test(id)) || !isTemplateId(template.id)) {
        return Promise.reject(new AiEffectsError('invalid'));
      }
      const body: Schemas['CreateAiJobRequest'] = {
        client_job_id: clientJobId,
        template_id: template.id,
        template_version: template.version,
        input_media_id: inputMediaId,
      };
      return read(parseJob, '/v1/ai-effects/jobs', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'Idempotency-Key': clientJobId },
        body: JSON.stringify(body),
      });
    },
    async getJob(id) {
      return read(parseJob, `/v1/ai-effects/jobs/${checkedId(id)}`, { method: 'GET' });
    },
    async mediaState(id) {
      return read(parseMediaState, `/v1/media/${checkedId(id)}`, { method: 'GET' });
    },
  };
}
