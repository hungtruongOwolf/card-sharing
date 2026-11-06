import type { components } from '@card-sharing/api-client';

import { problemCode } from '../../data/problemCode';
import type { AuthClient, IdTokenProvider } from '../auth';

type ReauthRequest = components['schemas']['ReauthRequest'];

/** A fresh provider proof from the native sheet (the same shape sign-in uses). */
export interface ReauthProof {
  readonly provider: IdTokenProvider;
  readonly idToken: string;
  readonly nonce: string;
}

/**
 * A re-auth token (T-26): sent as `X-Reauth` next to the session to `re` routes. It is bound to this device
 * session, lives 5 minutes and is never a bearer token. The app keeps it in memory for the one action it
 * was asked for and never stores it.
 */
export interface ReauthGrant {
  readonly token: string;
  /** Epoch milliseconds. */
  readonly expiresAt: number;
}

/** A pending account deletion (`DeletionView`). */
export interface PendingDeletion {
  /** Epoch milliseconds. */
  readonly requestedAt: number;
  /** Epoch milliseconds: until then signing in again and cancelling restore the account. */
  readonly graceUntil: number;
}

export type ExportStatus =
  'queued' | 'building' | 'ready' | 'downloaded' | 'expired' | 'cancelled' | 'failed';

/** A data export as its owner sees it (`ExportView`). */
export interface DataExport {
  readonly id: string;
  readonly status: ExportStatus;
  /** Epoch milliseconds. */
  readonly requestedAt: number;
  /** Epoch milliseconds: not built before this (48 h after the request); cancellable until then. */
  readonly availableAt: number;
  /** Epoch milliseconds, once ready. */
  readonly expiresAt: number | null;
  /** `ready` only: a single-use path to append to the API base URL. */
  readonly downloadPath: string | null;
}

export type PrivacyErrorKind =
  /** 401 `UNAUTHENTICATED` on `/v1/auth/reauth`: the proof was refused (other account, replayed, expired). */
  | 'proof_rejected'
  /** 401 `REAUTH_REQUIRED`: the re-auth token is missing, wrong or older than 5 minutes. */
  | 'reauth_required'
  /** 404: nothing pending, the grace period is over, or no such export of the caller. */
  | 'not_found'
  /** 429 `EXPORT_COOLDOWN`: one export per 24 hours. */
  | 'cooldown'
  /** 429 `RATE_LIMITED`. */
  | 'rate_limited'
  /** Network, 5xx, unexpected status or body; retry later. */
  | 'unavailable';

export class PrivacyError extends Error {
  override name = 'PrivacyError';
  constructor(
    readonly kind: PrivacyErrorKind,
    readonly status?: number,
    /** Seconds from `Retry-After` (cooldown and rate limits), when the server sent one. */
    readonly retryAfterSeconds?: number,
  ) {
    super(status === undefined ? `privacy: ${kind}` : `privacy: ${kind} (${status})`);
  }
}

/**
 * Account deletion (T-151) and data export (T-154) over the authorized `fetch` of the auth client. The server
 * decides everything (re-auth freshness, grace period, cooldown, ownership); this module only validates the
 * untrusted bodies and maps failures to kinds. A revoked session surfaces as `AuthError` from the fetcher.
 */
export interface PrivacyApi {
  /** `POST /v1/auth/reauth`: exchanges a fresh provider proof for a 5-minute re-auth token. */
  reauthenticate(proof: ReauthProof): Promise<ReauthGrant>;
  /** `GET /v1/me/deletion`: the pending deletion, or null when nothing is pending. */
  getDeletion(): Promise<PendingDeletion | null>;
  /** `POST /v1/me/deletion` (`re`): every session of the account, this one included, is logged out. */
  requestDeletion(grant: ReauthGrant): Promise<PendingDeletion>;
  /** `DELETE /v1/me/deletion`: cancels within the grace period (404 afterwards). */
  cancelDeletion(): Promise<void>;
  /** `POST /v1/me/exports` (`re`): queues an export, built 48 hours later. */
  requestExport(grant: ReauthGrant): Promise<DataExport>;
  /** `GET /v1/me/exports/{id}` (`re`): the status; when ready, a fresh single-use download path. */
  getExport(id: string, grant: ReauthGrant): Promise<DataExport>;
  /** `DELETE /v1/me/exports/{id}`: cancels while the export still waits for its delay. */
  cancelExport(id: string): Promise<void>;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const EXPORT_STATUSES: readonly ExportStatus[] = [
  'queued',
  'building',
  'ready',
  'downloaded',
  'expired',
  'cancelled',
  'failed',
];
/** Only a path on our own API, in the shape the server issues; anything else is never opened. */
const DOWNLOAD_PATH = /^\/v1\/exports\/[0-9a-f-]{36}\/download\?token=[A-Za-z0-9_-]{16,256}$/;
const MAX_TOKEN_LENGTH = 512;

const time = (value: unknown): number =>
  typeof value === 'string' ? Date.parse(value) : Number.NaN;

const field = (body: unknown, key: string): unknown =>
  typeof body === 'object' && body !== null && key in body
    ? (body as Record<string, unknown>)[key]
    : undefined;

export function parseReauthGrant(body: unknown): ReauthGrant | null {
  const token = field(body, 'reauth_token');
  const expiresAt = time(field(body, 'expires_at'));
  if (typeof token !== 'string' || token.length === 0 || token.length > MAX_TOKEN_LENGTH)
    return null;
  if (!Number.isFinite(expiresAt)) return null;
  return { token, expiresAt };
}

export function parseDeletion(body: unknown): PendingDeletion | null {
  const deletion = field(body, 'deletion');
  const requestedAt = time(field(deletion, 'requested_at'));
  const graceUntil = time(field(deletion, 'grace_until'));
  if (!Number.isFinite(requestedAt) || !Number.isFinite(graceUntil)) return null;
  return { requestedAt, graceUntil };
}

export function parseExport(body: unknown): DataExport | null {
  const view = field(body, 'export');
  const id = field(view, 'id');
  const status = EXPORT_STATUSES.find((s) => s === field(view, 'status'));
  const requestedAt = time(field(view, 'requested_at'));
  const availableAt = time(field(view, 'available_at'));
  if (typeof id !== 'string' || !UUID.test(id) || !status) return null;
  if (!Number.isFinite(requestedAt) || !Number.isFinite(availableAt)) return null;
  const expiresAt = time(field(view, 'expires_at'));
  const path = field(view, 'download_path');
  return {
    id,
    status,
    requestedAt,
    availableAt,
    expiresAt: Number.isFinite(expiresAt) ? expiresAt : null,
    downloadPath:
      status === 'ready' && typeof path === 'string' && DOWNLOAD_PATH.test(path) ? path : null,
  };
}

async function failure(response: Response): Promise<PrivacyError> {
  const { status } = response;
  const code = await problemCode(response);
  const retryAfter = Number(response.headers.get('Retry-After'));
  const seconds = Number.isFinite(retryAfter) && retryAfter > 0 ? retryAfter : undefined;
  if (code === 'REAUTH_REQUIRED') return new PrivacyError('reauth_required', status);
  if (code === 'EXPORT_COOLDOWN') return new PrivacyError('cooldown', status, seconds);
  if (status === 404) return new PrivacyError('not_found', status);
  if (status === 429) return new PrivacyError('rate_limited', status, seconds);
  return new PrivacyError('unavailable', status);
}

function parsed<T>(value: T | null, status: number): T {
  if (value === null) throw new PrivacyError('unavailable', status);
  return value;
}

export function createPrivacyApi(client: Pick<AuthClient, 'fetch' | 'fetchWithProof'>): PrivacyApi {
  const fetcher = client.fetch;
  const proofFetcher = client.fetchWithProof;
  const withReauth = (grant: ReauthGrant): Record<string, string> => ({ 'X-Reauth': grant.token });
  const exportPath = (id: string) => {
    // Only ids the server gave us; anything else is the same "not found" as the server's 404.
    if (!UUID.test(id)) throw new PrivacyError('not_found');
    return `/v1/me/exports/${encodeURIComponent(id)}`;
  };

  return {
    async reauthenticate(proof) {
      const body: ReauthRequest = {
        provider: proof.provider,
        id_token: proof.idToken,
        nonce: proof.nonce,
      };
      // A refused proof is 401 UNAUTHENTICATED on a live session: `fetchWithProof` returns it instead of
      // treating it as a stale session, so a wrong proof never signs the device out.
      const response = await proofFetcher('/v1/auth/reauth', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (response.status === 401) throw new PrivacyError('proof_rejected', 401);
      if (!response.ok) throw await failure(response);
      return parsed(parseReauthGrant(await response.json().catch(() => null)), response.status);
    },
    async getDeletion() {
      const response = await fetcher('/v1/me/deletion', { method: 'GET' });
      if (response.status === 404) return null;
      if (!response.ok) throw await failure(response);
      return parsed(parseDeletion(await response.json().catch(() => null)), response.status);
    },
    async requestDeletion(grant) {
      const response = await fetcher('/v1/me/deletion', {
        method: 'POST',
        headers: withReauth(grant),
      });
      if (!response.ok) throw await failure(response);
      return parsed(parseDeletion(await response.json().catch(() => null)), response.status);
    },
    async cancelDeletion() {
      const response = await fetcher('/v1/me/deletion', { method: 'DELETE' });
      if (!response.ok) throw await failure(response);
    },
    async requestExport(grant) {
      const response = await fetcher('/v1/me/exports', {
        method: 'POST',
        headers: withReauth(grant),
      });
      if (!response.ok) throw await failure(response);
      return parsed(parseExport(await response.json().catch(() => null)), response.status);
    },
    async getExport(id, grant) {
      const response = await fetcher(exportPath(id), { method: 'GET', headers: withReauth(grant) });
      if (!response.ok) throw await failure(response);
      return parsed(parseExport(await response.json().catch(() => null)), response.status);
    },
    async cancelExport(id) {
      const response = await fetcher(exportPath(id), { method: 'DELETE' });
      if (!response.ok) throw await failure(response);
    },
  };
}
