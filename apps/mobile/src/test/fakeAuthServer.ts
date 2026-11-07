/**
 * In-memory stand-in for the Rust session endpoints, used as the `fetch` of the auth client in tests.
 *
 * It mirrors crates/identity/src/service.rs (T-18) rather than a happy-path stub: opaque access tokens that
 * expire, single-use refresh tokens with generations, a 30 s grace for a retry of the immediately previous
 * token while its successor is unused, reuse detection that revokes the family, and the same 401 codes
 * (`UNAUTHENTICATED`, `SESSION_EXPIRED`, `SESSION_REVOKED`). Session management mirrors apps/api/src/routes/
 * sessions.rs (T-24): `GET /v1/sessions` lists the live sessions, most recently used first; `DELETE /v1/sessions/{id}`
 * answers the same 404 for a missing, revoked or malformed id; `POST /v1/sessions/revoke-all` ends every session
 * including the caller's. The fake has one account, so every family belongs to the caller. Requests are intercepted at the network
 * boundary (the injected `fetch`), the same seam MSW would use.
 */

import { pkceChallenge } from '../features/auth/pkce';

const ACCESS_TTL_MS = 10 * 60_000;
const REFRESH_GRACE_MS = 30_000;
const MAGIC_LINK_TTL_MS = 15 * 60_000;

/** A link `POST /v1/auth/email/start` "emailed" (crates/identity/src/magic_link.rs, ADR-0012). */
export interface FakeMagicLink {
  readonly requestId: string;
  readonly token: string;
  readonly email: string;
  readonly challenge: string;
  readonly createdAt: number;
  used: boolean;
}

interface RefreshToken {
  token: string;
  generation: number;
  consumedAt: number | null;
}

/** What `GET /v1/sessions` reports about a device session (crates/identity, T-24). */
export interface DeviceMeta {
  clientLabel?: string | null;
  ipCountry?: string | null;
  /** Epoch milliseconds; defaults to the issue time. */
  lastSeenAt?: number;
}

interface Family {
  id: number;
  /** Public session id (`SessionView.id`). */
  sessionId: string;
  meta: Required<DeviceMeta>;
  createdAt: number;
  revoked: boolean;
  accessToken: string;
  accessExpiresAt: number;
  refreshTokens: RefreshToken[];
}

export interface RecordedRequest {
  method: string;
  path: string;
  authorization: string | null;
  appVersion: string | null;
  idempotencyKey: string | null;
  /** `X-Reauth` (T-26). */
  reauth: string | null;
  contentType: string | null;
  /** Parsed JSON for string bodies. */
  body: unknown;
  /** The body as sent (e.g. the bytes of a media upload). */
  rawBody: unknown;
}

export const problem = (status: number, code: string) =>
  new Response(JSON.stringify({ type: 'about:blank', status, code }), {
    status,
    headers: { 'Content-Type': 'application/problem+json' },
  });

export const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });

export class FakeAuthServer {
  readonly baseUrl = 'https://api.test';
  readonly requests: RecordedRequest[] = [];
  /** Responses to return instead of handling a refresh (e.g. 503), consumed in order. */
  readonly refreshOverrides: (Response | 'network-error' | 'lose-response')[] = [];
  nowMs = Date.parse('2026-11-05T10:00:00Z');
  /**
   * Extra authorized routes (see fakeOnboardingServer.ts). Called after the bearer token is accepted;
   * return null to fall through to the built-in routes.
   */
  routes: ((request: RecordedRequest) => Response | null) | null = null;
  /**
   * Seed handles the development sign-in accepts (#306, apps/api/src/routes/dev_auth.rs); null = the server has no
   * development sign-in, so `/v1/dev/*` answers 404 like staging and production.
   */
  devSeedHandles: Set<string> | null = null;
  /** Magic links sent, newest last. */
  readonly magicLinks: FakeMagicLink[] = [];
  /** Emails with an account (`POST /v1/auth/email/verify` signs them in without an invite code). */
  readonly emailAccounts = new Set<string>();
  /** The sign-up gate (`config/signup.yaml` `invites.required`). */
  inviteRequired = false;
  /** Invite codes the gate accepts (canonical, without dashes), each with uses left. */
  readonly inviteCodes = new Map<string, number>();

  private families: Family[] = [];
  private counter = 0;
  private refreshGate: Promise<void> | null = null;

  now = (): number => this.nowMs;

  advance(ms: number): void {
    this.nowMs += ms;
  }

  /** Mints a session as a sign-in route would and returns the token pair body. */
  issue(meta: DeviceMeta = {}) {
    const id = this.families.length + 1;
    const family: Family = {
      id,
      sessionId: `00000000-0000-4000-8000-${String(id).padStart(12, '0')}`,
      meta: {
        clientLabel: meta.clientLabel ?? null,
        ipCountry: meta.ipCountry ?? null,
        lastSeenAt: meta.lastSeenAt ?? this.nowMs,
      },
      createdAt: this.nowMs,
      revoked: false,
      accessToken: this.token('access'),
      accessExpiresAt: this.nowMs + ACCESS_TTL_MS,
      refreshTokens: [{ token: this.token('refresh'), generation: 1, consumedAt: null }],
    };
    this.families.push(family);
    return this.pair(family);
  }

  /** The newest link sent to `email`. */
  linkFor(email: string): FakeMagicLink {
    const link = [...this.magicLinks].reverse().find((l) => l.email === email);
    if (!link) throw new Error(`no magic link for ${email}`);
    return link;
  }

  /** The server forgets every session (e.g. the account was deleted): their tokens become unknown. */
  dropSessions(): void {
    this.families = [];
  }

  /** Server-side revocation (logout everywhere, suspension, ban). */
  revokeAll(): void {
    for (const f of this.families) f.revoked = true;
  }

  /** Holds every refresh until the returned function is called, so concurrent callers pile up. */
  holdRefreshes(): () => void {
    let release!: () => void;
    this.refreshGate = new Promise<void>((resolve) => {
      release = resolve;
    });
    return () => {
      this.refreshGate = null;
      release();
    };
  }

  count(method: string, path: string): number {
    return this.requests.filter((r) => r.method === method && r.path === path).length;
  }

  isRevoked(familyId = 1): boolean {
    return this.families[familyId - 1]?.revoked ?? false;
  }

  /** Public session id of a family, as listed by `GET /v1/sessions`. */
  sessionId(familyId = 1): string {
    const family = this.families[familyId - 1];
    if (!family) throw new Error(`no session ${familyId}`);
    return family.sessionId;
  }

  readonly fetch: typeof fetch = async (input, init) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    const path = url.startsWith(this.baseUrl) ? url.slice(this.baseUrl.length) : url;
    const method = (init?.method ?? 'GET').toUpperCase();
    const headers = new Headers(init?.headers);
    const body: unknown = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined;
    const recorded: RecordedRequest = {
      method,
      path,
      authorization: headers.get('Authorization'),
      appVersion: headers.get('X-App-Version'),
      idempotencyKey: headers.get('Idempotency-Key'),
      reauth: headers.get('X-Reauth'),
      contentType: headers.get('Content-Type'),
      body,
      rawBody: init?.body,
    };
    this.requests.push(recorded);
    // Let concurrent callers interleave like real network I/O.
    await Promise.resolve();

    if (method === 'POST' && path === '/v1/auth/refresh') return this.refresh(body);
    if (method === 'POST' && (path === '/v1/auth/apple' || path === '/v1/auth/google')) {
      const b = body as { id_token?: unknown; nonce?: unknown; invite_code?: unknown } | undefined;
      if (b?.id_token !== 'valid-id-token' || typeof b.nonce !== 'string' || b.nonce.length < 16) {
        return problem(401, 'UNAUTHENTICATED');
      }
      // The fake signs every provider token into a new account, so the sign-up gate always applies.
      const refusal = this.inviteRefusal(b.invite_code);
      if (refusal) return refusal;
      return json(this.issue());
    }

    if (method === 'POST' && path === '/v1/auth/email/start') return this.startMagicLink(body);
    if (method === 'POST' && path === '/v1/auth/email/verify') return this.verifyMagicLink(body);

    if (path.startsWith('/v1/dev/')) {
      if (!this.devSeedHandles) return problem(404, 'NOT_FOUND');
      if (method === 'GET' && path === '/v1/dev/seed-users') {
        return json({
          users: [...this.devSeedHandles].map((handle) => ({
            handle,
            display_name: `${handle.split('.')[0]} (demo)`,
            minor: handle === 'mia.demo',
          })),
        });
      }
      if (method === 'POST' && path === '/v1/dev/sign-in') {
        const handle = (body as { handle?: unknown } | undefined)?.handle;
        if (typeof handle !== 'string' || !this.devSeedHandles.has(handle)) {
          return problem(404, 'NOT_FOUND');
        }
        return json(this.issue());
      }
      return problem(404, 'NOT_FOUND');
    }

    const resolved = this.resolve(headers.get('Authorization'));
    if (resolved instanceof Response) return resolved;
    const routed = this.routes?.(recorded);
    if (routed) return routed;
    if (method === 'GET' && path === '/v1/me') return json({ account: { id: 'acc-1' } });
    if (method === 'DELETE' && path === '/v1/sessions/current') {
      resolved.revoked = true;
      return new Response(null, { status: 204 });
    }
    resolved.meta.lastSeenAt = this.nowMs;
    if (method === 'GET' && path === '/v1/sessions') return json(this.listSessions(resolved));
    if (method === 'POST' && path === '/v1/sessions/revoke-all') {
      this.revokeAll();
      return new Response(null, { status: 204 });
    }
    const revoke = method === 'DELETE' ? /^\/v1\/sessions\/([^/]+)$/.exec(path) : null;
    if (revoke) {
      const target = this.families.find(
        (f) => !f.revoked && f.sessionId === decodeURIComponent(revoke[1] ?? ''),
      );
      if (!target) return problem(404, 'NOT_FOUND');
      target.revoked = true;
      return new Response(null, { status: 204 });
    }
    return problem(404, 'NOT_FOUND');
  };

  private listSessions(caller: Family) {
    const sessions = this.families
      .filter((f) => !f.revoked)
      .sort((a, b) => b.meta.lastSeenAt - a.meta.lastSeenAt)
      .map((f) => ({
        id: f.sessionId,
        client_label: f.meta.clientLabel,
        ip_country: f.meta.ipCountry,
        created_at: new Date(f.createdAt).toISOString(),
        last_seen_at: new Date(f.meta.lastSeenAt).toISOString(),
        is_current: f === caller,
      }));
    return { sessions };
  }

  private token(kind: string): string {
    this.counter += 1;
    return `${kind}-${this.counter}`;
  }

  private pair(family: Family) {
    const latest = family.refreshTokens[family.refreshTokens.length - 1];
    return {
      token_type: 'Bearer',
      access_token: family.accessToken,
      access_expires_at: new Date(family.accessExpiresAt).toISOString(),
      refresh_token: latest?.token ?? '',
    };
  }

  private resolve(authorization: string | null): Family | Response {
    const token = authorization?.startsWith('Bearer ') ? authorization.slice(7) : null;
    const family = this.families.find((f) => f.accessToken === token);
    if (!family) return problem(401, 'UNAUTHENTICATED');
    if (family.revoked) return problem(401, 'SESSION_REVOKED');
    if (this.nowMs >= family.accessExpiresAt) return problem(401, 'SESSION_EXPIRED');
    return family;
  }

  /** The sign-up gate for a sign-in that creates an account: null when it may proceed (one use spent). */
  private inviteRefusal(inviteCode: unknown): Response | null {
    if (!this.inviteRequired) return null;
    if (typeof inviteCode !== 'string') return problem(403, 'INVITE_REQUIRED');
    const code = inviteCode.toUpperCase().replace(/[\s-]/g, '');
    const left = this.inviteCodes.get(code) ?? 0;
    if (left < 1) return problem(403, 'INVITE_INVALID');
    this.inviteCodes.set(code, left - 1);
    return null;
  }

  private startMagicLink(body: unknown): Response {
    const b = body as { email?: unknown; code_challenge?: unknown } | undefined;
    if (
      typeof b?.email !== 'string' ||
      !b.email.includes('@') ||
      typeof b.code_challenge !== 'string'
    ) {
      return problem(400, 'VALIDATION_FAILED');
    }
    this.counter += 1;
    const n = String(this.counter).padStart(12, '0');
    const link: FakeMagicLink = {
      requestId: `00000000-0000-4000-9000-${n}`,
      token: `tok${n}`.padEnd(43, 'x'),
      email: b.email.trim().toLowerCase(),
      challenge: b.code_challenge,
      createdAt: this.nowMs,
      used: false,
    };
    this.magicLinks.push(link);
    return json({ request_id: link.requestId }, 202);
  }

  private verifyMagicLink(body: unknown): Response {
    const b = body as
      { token?: unknown; code_verifier?: unknown; invite_code?: unknown } | undefined;
    const link = this.magicLinks.find((l) => l.token === b?.token);
    const verifier = typeof b?.code_verifier === 'string' ? b.code_verifier : '';
    if (!link || pkceChallenge(verifier) !== link.challenge)
      return problem(401, 'MAGIC_LINK_INVALID');
    if (link.used) return problem(410, 'MAGIC_LINK_USED');
    if (this.nowMs - link.createdAt >= MAGIC_LINK_TTL_MS) return problem(410, 'MAGIC_LINK_EXPIRED');
    if (!this.emailAccounts.has(link.email)) {
      const refusal = this.inviteRefusal(b?.invite_code);
      if (refusal) return refusal;
    }
    link.used = true;
    this.emailAccounts.add(link.email);
    return json(this.issue());
  }

  private async refresh(body: unknown): Promise<Response> {
    if (this.refreshGate) await this.refreshGate;
    const override = this.refreshOverrides.shift();
    if (override === 'network-error') throw new TypeError('Network request failed');
    if (override instanceof Response) return override;

    const presented = (body as { refresh_token?: unknown } | undefined)?.refresh_token;
    let family: Family | undefined;
    let row: RefreshToken | undefined;
    for (const f of this.families) {
      row = f.refreshTokens.find((t) => t.token === presented);
      if (row) {
        family = f;
        break;
      }
    }
    if (!family || !row) return problem(401, 'UNAUTHENTICATED');
    if (family.revoked) return problem(401, 'SESSION_REVOKED');

    const latest = family.refreshTokens[family.refreshTokens.length - 1] as RefreshToken;
    let toConsume: RefreshToken;
    if (row.consumedAt === null && row.generation === latest.generation) {
      toConsume = row;
    } else if (
      row.consumedAt !== null &&
      row.generation === latest.generation - 1 &&
      latest.consumedAt === null &&
      this.nowMs - row.consumedAt <= REFRESH_GRACE_MS
    ) {
      toConsume = latest; // retry of the previous token: supersede the unused successor
    } else {
      family.revoked = true; // reuse detected: revoke the whole family
      return problem(401, 'SESSION_REVOKED');
    }

    toConsume.consumedAt = this.nowMs;
    family.accessToken = this.token('access');
    family.accessExpiresAt = this.nowMs + ACCESS_TTL_MS;
    family.refreshTokens.push({
      token: this.token('refresh'),
      generation: latest.generation + 1,
      consumedAt: null,
    });
    const response = json(this.pair(family));
    if (override === 'lose-response') throw new TypeError('Network request failed');
    return response;
  }
}
