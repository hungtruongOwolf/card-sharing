/**
 * Share tokens are opaque, unguessable, URL-safe strings issued by the API.
 * Shape check only — authorization (revoked? expired? board visibility?) is the API's job.
 */
const SHARE_TOKEN = /^[A-Za-z0-9_-]{16,64}$/;

export function isShareToken(value: string): boolean {
  return SHARE_TOKEN.test(value);
}
