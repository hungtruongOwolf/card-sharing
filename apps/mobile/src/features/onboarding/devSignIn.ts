import type { components } from '@card-sharing/api-client';

import { problemCode } from '../../data/problemCode';
import { SERVER_BUSY } from '../../data/serverBusy';
import type { AuthClient } from '../auth';

type DevSeedUsers = components['schemas']['DevSeedUsers'];

/** A seed user a local development server can sign in (`GET /v1/dev/seed-users`). */
export interface SeedUser {
  readonly handle: string;
  readonly displayName: string;
  readonly minor: boolean;
}

export type DevSignInErrorKind =
  /** 404: this server has no development sign-in (staging, production, or `API_DEV_SIGN_IN` off). */
  | 'disabled'
  /** 503 `SERVER_BUSY`: the server is overloaded, not down. */
  | 'busy'
  /** Network, 429, 5xx or an unexpected body. */
  | 'unavailable';

export class DevSignInError extends Error {
  override name = 'DevSignInError';
  constructor(readonly kind: DevSignInErrorKind) {
    super(`dev sign-in: ${kind}`);
  }
}

/**
 * Development-only sign-in as a seed user (#306), for demos against a local API before real Apple/Google accounts
 * exist. The app creates it only when `__DEV__` is true (release builds get null and render no entry); the server is
 * the real gate: outside `API_ENV=development|test` with `API_DEV_SIGN_IN=true` the routes do not exist (404), and
 * only the seed personas can be signed into.
 */
export interface DevSignIn {
  listSeedUsers(): Promise<SeedUser[]>;
  /** Signs in and stores the session like a provider sign-in; the onboarding flow continues from `/v1/me`. */
  signIn(handle: string): Promise<void>;
}

const HANDLE = /^[a-z0-9._]{1,32}$/;

export function parseSeedUsers(body: unknown): SeedUser[] | null {
  if (typeof body !== 'object' || body === null || !('users' in body)) return null;
  const { users } = body as { users: unknown };
  if (!Array.isArray(users)) return null;
  return users.flatMap((value: unknown) => {
    if (typeof value !== 'object' || value === null) return [];
    const u = value as Partial<Record<keyof DevSeedUsers['users'][number], unknown>>;
    if (typeof u.handle !== 'string' || !HANDLE.test(u.handle)) return [];
    if (typeof u.display_name !== 'string' || typeof u.minor !== 'boolean') return [];
    return [{ handle: u.handle, displayName: u.display_name.slice(0, 64), minor: u.minor }];
  });
}

export interface DevSignInOptions {
  baseUrl: () => string;
  authClient: Pick<AuthClient, 'signInAsSeedUser'>;
  fetch?: typeof fetch;
}

export function createDevSignIn({
  baseUrl,
  authClient,
  fetch: fetchFn = (input, init) => fetch(input, init),
}: DevSignInOptions): DevSignIn {
  return {
    async listSeedUsers() {
      let response: Response;
      try {
        response = await fetchFn(`${baseUrl()}/v1/dev/seed-users`, {
          method: 'GET',
          headers: { Accept: 'application/json' },
        });
      } catch {
        throw new DevSignInError('unavailable');
      }
      if (response.status === 404) throw new DevSignInError('disabled');
      if (response.status === 503 && (await problemCode(response)) === SERVER_BUSY) {
        throw new DevSignInError('busy');
      }
      if (!response.ok) throw new DevSignInError('unavailable');
      const users = parseSeedUsers(await response.json().catch(() => null));
      if (!users) throw new DevSignInError('unavailable');
      return users;
    },
    signIn: (handle) => authClient.signInAsSeedUser(handle),
  };
}
