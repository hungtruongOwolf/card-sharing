/**
 * In-memory stand-in for the Rust session endpoints, used as the `fetch` of the auth client in tests.
 *
 * It mirrors crates/identity/src/service.rs (T-18) rather than a happy-path stub: opaque access tokens that
 * expire, single-use refresh tokens with generations, a 30 s grace for a retry of the immediately previous
 * token while its successor is unused, reuse detection that revokes the family, and the same 401 codes
 * (`UNAUTHENTICATED`, `SESSION_EXPIRED`, `SESSION_REVOKED`). Requests are intercepted at the network
 * boundary (the injected `fetch`), the same seam MSW would use.
 */

const ACCESS_TTL_MS = 10 * 60_000;
const REFRESH_GRACE_MS = 30_000;

interface RefreshToken {
  token: string;
  generation: number;
  consumedAt: number | null;
}

interface Family {
  id: number;
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
  body: unknown;
}

const problem = (status: number, code: string) =>
  new Response(JSON.stringify({ type: 'about:blank', status, code }), {
    status,
    headers: { 'Content-Type': 'application/problem+json' },
  });

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });

export class FakeAuthServer {
  readonly baseUrl = 'https://api.test';
  readonly requests: RecordedRequest[] = [];
  /** Responses to return instead of handling a refresh (e.g. 503), consumed in order. */
  readonly refreshOverrides: (Response | 'network-error' | 'lose-response')[] = [];
  nowMs = Date.parse('2026-11-05T10:00:00Z');

  private families: Family[] = [];
  private counter = 0;
  private refreshGate: Promise<void> | null = null;

  now = (): number => this.nowMs;

  advance(ms: number): void {
    this.nowMs += ms;
  }

  /** Mints a session as a sign-in route would and returns the token pair body. */
  issue() {
    const family: Family = {
      id: this.families.length + 1,
      revoked: false,
      accessToken: this.token('access'),
      accessExpiresAt: this.nowMs + ACCESS_TTL_MS,
      refreshTokens: [{ token: this.token('refresh'), generation: 1, consumedAt: null }],
    };
    this.families.push(family);
    return this.pair(family);
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

  readonly fetch: typeof fetch = async (input, init) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    const path = url.startsWith(this.baseUrl) ? url.slice(this.baseUrl.length) : url;
    const method = (init?.method ?? 'GET').toUpperCase();
    const headers = new Headers(init?.headers);
    const body: unknown = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined;
    this.requests.push({
      method,
      path,
      authorization: headers.get('Authorization'),
      appVersion: headers.get('X-App-Version'),
      body,
    });
    // Let concurrent callers interleave like real network I/O.
    await Promise.resolve();

    if (method === 'POST' && path === '/v1/auth/refresh') return this.refresh(body);
    if (method === 'POST' && (path === '/v1/auth/apple' || path === '/v1/auth/google')) {
      const b = body as { id_token?: unknown; nonce?: unknown } | undefined;
      if (b?.id_token !== 'valid-id-token' || typeof b.nonce !== 'string' || b.nonce.length < 16) {
        return problem(401, 'UNAUTHENTICATED');
      }
      return json(this.issue());
    }

    const resolved = this.resolve(headers.get('Authorization'));
    if (resolved instanceof Response) return resolved;
    if (method === 'GET' && path === '/v1/me') return json({ account: { id: 'acc-1' } });
    if (method === 'DELETE' && path === '/v1/sessions/current') {
      resolved.revoked = true;
      return new Response(null, { status: 204 });
    }
    return problem(404, 'NOT_FOUND');
  };

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
