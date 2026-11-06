import type { Env } from '../service-api/service-api.ts';

/**
 * Captcha provider choice for guest reports (T-142, SR-31). The **API** verifies the widget token
 * (`captcha_token` on `POST /v1/public/reports`, its own captcha port): one enforcement point, and
 * the vendor secret lives only in the API. The web server only picks which widget to render (plus,
 * for a real vendor later, its public site key and CSP origins); it holds no captcha secret.
 *
 * `CAPTCHA_PROVIDER` (server env): empty = no widget, guest reports are off (deny by default);
 * `fake` = a development checkbox that yields `FAKE_CAPTCHA_TOKEN`, refused in production builds.
 * The API accepts that token only with `API_CAPTCHA_VERIFIER=fake` (refused in production).
 * The vendor is the EM's choice.
 */

export type CaptchaProvider = 'fake';

/** The token the fake widget hands out once its box is ticked (the API's fake verifier accepts it). */
export const FAKE_CAPTCHA_TOKEN = 'fake-captcha-pass';

/** Bound on a token from the browser (the API's `captcha_token` maxLength). */
export const MAX_CAPTCHA_TOKEN = 4096;

/** The configured widget provider, or `null` (reports off). `fake` never runs in a production build. */
export function captchaProvider(env: Env): CaptchaProvider | null {
  const name = env.CAPTCHA_PROVIDER?.trim().toLowerCase() ?? '';
  if (name === 'fake') {
    return env.NODE_ENV === 'production' ? null : 'fake';
  }
  return null;
}
