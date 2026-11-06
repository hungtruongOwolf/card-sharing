import type { Env } from '../service-api/service-api.ts';

/**
 * Captcha port (T-142, SR-31). The web server verifies a guest's captcha answer before it forwards
 * a report to the API, so the vendor secret never leaves the web server and the API stays
 * vendor-neutral. Providers are a registry: a real vendor adds a verifier here, a widget in
 * `CaptchaField.tsx` and its script/frame origins in the CSP. The vendor is the EM's choice; only
 * the development `fake` provider exists today.
 *
 * `CAPTCHA_PROVIDER` (server env): empty = no provider, guest reports are off (deny by default);
 * `fake` = a checkbox that always passes, refused in production builds.
 */

export type CaptchaProvider = 'fake';

export interface CaptchaVerifier {
  /** `true` only for an answer the provider accepted for this visitor. Never throws. */
  verify(answer: string, remoteIp: string): Promise<boolean>;
}

/** The answer the fake widget submits once its box is ticked. */
export const FAKE_CAPTCHA_ANSWER = 'fake-captcha-pass';

/** Bound on an answer from the browser (vendor tokens are about 2 KB at most). */
export const MAX_CAPTCHA_ANSWER = 4096;

const fakeVerifier: CaptchaVerifier = {
  async verify(answer) {
    return answer === FAKE_CAPTCHA_ANSWER;
  },
};

const VERIFIERS: Record<CaptchaProvider, CaptchaVerifier> = { fake: fakeVerifier };

/** The configured provider, or `null` (reports off). `fake` never runs in a production build. */
export function captchaProvider(env: Env): CaptchaProvider | null {
  const name = env.CAPTCHA_PROVIDER?.trim().toLowerCase() ?? '';
  if (name === 'fake') {
    return env.NODE_ENV === 'production' ? null : 'fake';
  }
  return null;
}

export function captchaVerifier(provider: CaptchaProvider): CaptchaVerifier {
  return VERIFIERS[provider];
}
