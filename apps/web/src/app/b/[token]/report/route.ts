import { captchaProvider } from '../../../../features/captcha/captcha';
import {
  OUTCOME_STATUS,
  isSameOriginJson,
  parseReportRequest,
  submitReport,
} from '../../../../features/public-board/report';
import type { ReportOutcome } from '../../../../features/public-board/report-shared';
import { readServiceApiConfig, visitorIp } from '../../../../features/service-api/service-api';
import { siteUrl } from '../../../../site';

type Context = { params: Promise<{ token: string }> };

/** Far above a valid body (reason + 500-character note + captcha token). */
const MAX_BODY_BYTES = 16 * 1024;

function answer(outcome: ReportOutcome): Response {
  return Response.json(
    { outcome },
    { status: OUTCOME_STATUS[outcome], headers: { 'Cache-Control': 'private, no-store' } },
  );
}

/**
 * `POST /b/<token>/report` (T-142, AC-26.3): a guest reports a public board. Same-origin JSON only;
 * forwarded with the widget's captcha token to the API, which verifies the captcha server-side and
 * applies its 5-per-hour limit per visitor IP. Off unless a captcha widget provider is configured.
 */
export async function POST(request: Request, { params }: Context): Promise<Response> {
  const { token } = await params;
  const provider = captchaProvider(process.env);
  const config = readServiceApiConfig(process.env);
  if (provider === null || config === null) {
    return answer('unavailable');
  }
  if (!isSameOriginJson(request.headers, siteUrl().origin)) {
    return answer('invalid');
  }
  const declared = Number(request.headers.get('content-length') ?? '0');
  if (declared > MAX_BODY_BYTES) {
    return answer('invalid');
  }
  const text = await request.text();
  if (text.length > MAX_BODY_BYTES) {
    return answer('invalid');
  }
  let body: unknown;
  try {
    body = JSON.parse(text);
  } catch {
    return answer('invalid');
  }
  const report = parseReportRequest(body);
  if (report === null) {
    return answer('invalid');
  }
  if (report === 'no-captcha') {
    return answer('captcha');
  }
  // The API verifies the captcha token (single enforcement point) before it looks the link up.
  const ip = visitorIp(request.headers, config.clientIpHeader);
  return answer(await submitReport(token, report, config, ip));
}
