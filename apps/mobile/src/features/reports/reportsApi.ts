import type { components } from '@card-sharing/api-client';

import { problemCode } from '../../data/problemCode';
import type { AuthClient } from '../auth';

type ReportRequest = components['schemas']['ReportRequest'];

/**
 * What can be reported (US-26, `POST /v1/reports`, T-141). Only things the caller can see right now; the server
 * answers 404 for anything else, including the caller's own content.
 */
export type ReportTargetKind = components['schemas']['ReportTargetType'];

export interface ReportTarget {
  readonly kind: ReportTargetKind;
  readonly id: string;
}

/** AC-26.1 reasons, in the order the sheet shows them. */
export const REPORT_REASONS = [
  'sexual',
  'minor_safety',
  'harassment',
  'violence',
  'hate',
  'spam',
  'other',
] as const satisfies readonly components['schemas']['ReportReason'][];
export type ReportReason = (typeof REPORT_REASONS)[number];

export type ReportsErrorKind =
  /** 404: not visible to me (gone, never mine to see); the same answer as a target that does not exist. */
  | 'not_found'
  /** 429: 20 reports a day. */
  | 'rate_limited'
  | 'unavailable';

export class ReportsError extends Error {
  override name = 'ReportsError';
  constructor(
    readonly kind: ReportsErrorKind,
    readonly status?: number,
  ) {
    super(status === undefined ? `reports: ${kind}` : `reports: ${kind} (${status})`);
  }
}

export interface ReportsApi {
  /** Files a report; a repeat for the same target is deduplicated by the server and still succeeds. */
  report(target: ReportTarget, reason: ReportReason, idempotencyKey: string): Promise<void>;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
/** A public link is reported by the token from its URL (link ids are only shown to the owner). */
const LINK_TOKEN = /^[A-Za-z0-9_-]{16,64}$/;

const validTarget = ({ kind, id }: ReportTarget) =>
  kind === 'public_link' ? LINK_TOKEN.test(id) : UUID.test(id);

export function createReportsApi(fetcher: AuthClient['fetch']): ReportsApi {
  return {
    async report(target, reason, idempotencyKey) {
      if (!validTarget(target)) throw new ReportsError('not_found');
      const response = await fetcher('/v1/reports', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'Idempotency-Key': idempotencyKey },
        body: JSON.stringify({
          target: { type: target.kind, id: target.id },
          reason,
        } satisfies ReportRequest),
      });
      if (response.ok) return;
      const code = await problemCode(response);
      if (response.status === 404 || code === 'NOT_FOUND') throw new ReportsError('not_found', 404);
      if (response.status === 429) throw new ReportsError('rate_limited', 429);
      throw new ReportsError('unavailable', response.status);
    },
  };
}
