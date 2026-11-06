import type { components } from '@card-sharing/api-client';

import { MAX_CAPTCHA_TOKEN } from '../captcha/captcha.ts';
import { type ServiceApiConfig, servicePost } from '../service-api/service-api.ts';

import { isBoardToken } from './public-board.ts';
import {
  MAX_REPORT_NOTE,
  REPORT_REASONS,
  type ReportOutcome,
  type ReportReason,
} from './report-shared.ts';

/**
 * Guest report of a public board link (T-142, US-26 AC-26.3, SR-31). The browser posts to the web
 * server's `/b/<token>/report`; the web server checks the request and forwards it, with the
 * widget's captcha token, to the API's `POST /v1/public/reports` (`svc`, `X-Client-IP`). The API
 * verifies the captcha and limits 5 reports per hour per IP.
 */

export type ReportRequest = {
  readonly reason: ReportReason;
  readonly note: string | null;
  /** The widget token, forwarded unchanged as `captcha_token`. */
  readonly captchaToken: string;
};

/** HTTP status the web route answers for each outcome (AC: bad captcha 400, 6th report 429). */
export const OUTCOME_STATUS: Record<ReportOutcome, number> = {
  accepted: 202,
  invalid: 400,
  captcha: 400,
  gone: 404,
  limited: 429,
  unavailable: 503,
};

const REASONS: ReadonlySet<string> = new Set(REPORT_REASONS.map((reason) => reason.value));

/** Validates the browser's JSON body; `null` when unusable. A missing captcha is its own case. */
export function parseReportRequest(value: unknown): ReportRequest | 'no-captcha' | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null;
  const body = value as Record<string, unknown>;
  if (typeof body.reason !== 'string' || !REASONS.has(body.reason)) return null;
  let note: string | null = null;
  if (body.note !== undefined && body.note !== null) {
    if (typeof body.note !== 'string' || body.note.length > MAX_REPORT_NOTE) return null;
    note = body.note.trim() === '' ? null : body.note.trim();
  }
  const captcha = body.captcha_token;
  if (typeof captcha !== 'string' || captcha === '') return 'no-captcha';
  if (captcha.length > MAX_CAPTCHA_TOKEN) return null;
  return { reason: body.reason as ReportReason, note, captchaToken: captcha };
}

/**
 * The browser request must come from this site: a same-origin `Origin` header and a JSON body.
 * There are no cookies, so this is defence in depth against cross-site form posts.
 */
export function isSameOriginJson(headers: Headers, siteOrigin: string): boolean {
  const origin = headers.get('origin');
  const type = headers.get('content-type') ?? '';
  return origin === siteOrigin && /^application\/json(\s*;|$)/i.test(type);
}

/** The API's request body, exactly as the generated client types it. */
type PublicReportRequest = components['schemas']['PublicReportRequest'];

/**
 * Forwards a report to the API with the widget's captcha token; the API verifies the captcha.
 * Never throws.
 */
export async function submitReport(
  token: string,
  report: Pick<ReportRequest, 'reason' | 'note' | 'captchaToken'>,
  config: ServiceApiConfig,
  clientIp: string,
  fetchImpl: typeof fetch = fetch,
): Promise<ReportOutcome> {
  if (!isBoardToken(token)) return 'gone';
  const body: PublicReportRequest = {
    token,
    reason: report.reason,
    captcha_token: report.captchaToken,
    ...(report.note === null ? {} : { note: report.note }),
  };
  const response = await servicePost('/v1/public/reports', body, config, clientIp, fetchImpl);
  if (response === null) return 'unavailable';
  if (response.status === 202) return 'accepted';
  if (response.status === 404) return 'gone';
  if (response.status === 429) return 'limited';
  // The body was validated here first, so a 400 from the API means a missing or failed captcha.
  if (response.status === 400) return 'captcha';
  // 503: no captcha provider configured on the API, or the provider is down.
  return 'unavailable';
}
