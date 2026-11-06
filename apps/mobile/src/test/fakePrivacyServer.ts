/**
 * In-memory stand-in for the re-auth (T-26), account deletion (T-151) and data export (T-154) endpoints on
 * top of {@link FakeAuthServer} (same network-boundary seam). It mirrors apps/api/src/routes/auth.rs,
 * deletion.rs and exports.rs:
 * - `POST /v1/auth/reauth` accepts only the account's own fresh proof (`valid-id-token`, nonce 16+ chars,
 *   single use) and answers a 5-minute token; anything else is 401 `UNAUTHENTICATED`.
 * - `re` routes need the latest re-auth token in `X-Reauth`, younger than 5 minutes, else 401 `REAUTH_REQUIRED`.
 * - `POST /v1/me/deletion` revokes every session (this one too) and answers 202 with a 7-day grace.
 * - `POST /v1/me/exports` queues an export available 48 h later; a second one within 24 h is 429
 *   `EXPORT_COOLDOWN` with `Retry-After`. `GET` returns a fresh single-use download path once ready.
 */
import { FakeAuthServer, json, problem, type RecordedRequest } from './fakeAuthServer';

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
export const REAUTH_TTL_MS = 5 * MINUTE;
export const GRACE_MS = 7 * 24 * HOUR;
export const EXPORT_DELAY_MS = 48 * HOUR;
const EXPORT_COOLDOWN_MS = 24 * HOUR;

/** The n-th single-use download token the fake issues (a placeholder in the server's shape). */
export const downloadToken = (n: number) => `dl${'x'.repeat(16)}${n}`;

interface FakeExport {
  id: string;
  status: 'queued' | 'building' | 'ready' | 'downloaded' | 'expired' | 'cancelled' | 'failed';
  requestedAt: number;
  availableAt: number;
  expiresAt: number | null;
}

export class FakePrivacyServer {
  deletion: { requestedAt: number; graceUntil: number } | null = null;
  readonly exports: FakeExport[] = [];
  /** Called when a pending deletion is cancelled (e.g. to make the onboarding fake's account active again). */
  onDeletionCancelled: (() => void) | null = null;
  /** Next response for `METHOD path`, consumed once (e.g. a 503). */
  readonly overrides = new Map<string, Response>();

  private grant: { token: string; issuedAt: number } | null = null;
  private readonly usedNonces = new Set<string>();
  private counter = 0;
  private downloadCounter = 0;

  constructor(readonly auth: FakeAuthServer = new FakeAuthServer()) {
    const previous = auth.routes;
    auth.routes = (request) => this.route(request) ?? previous?.(request) ?? null;
  }

  get baseUrl(): string {
    return this.auth.baseUrl;
  }

  readonly fetch: typeof fetch = (input, init) => this.auth.fetch(input, init);

  /** Marks an export as built (what the worker does after `available_at`). */
  markReady(id: string): void {
    const item = this.exports.find((e) => e.id === id);
    if (!item) throw new Error(`no export ${id}`);
    item.status = 'ready';
    item.expiresAt = this.auth.nowMs + 24 * HOUR;
  }

  private iso = (ms: number | null) => (ms === null ? null : new Date(ms).toISOString());

  private reauthOk(request: RecordedRequest): boolean {
    return (
      this.grant !== null &&
      request.reauth === this.grant.token &&
      this.auth.nowMs - this.grant.issuedAt < REAUTH_TTL_MS
    );
  }

  private exportView(item: FakeExport, withDownload: boolean) {
    let downloadPath: string | null = null;
    if (withDownload && item.status === 'ready') {
      this.downloadCounter += 1;
      downloadPath = `/v1/exports/${item.id}/download?token=${downloadToken(this.downloadCounter)}`;
    }
    return {
      export: {
        id: item.id,
        status: item.status,
        requested_at: this.iso(item.requestedAt),
        available_at: this.iso(item.availableAt),
        expires_at: this.iso(item.expiresAt),
        download_path: downloadPath,
      },
    };
  }

  private deletionView() {
    if (!this.deletion) throw new Error('no deletion');
    return {
      deletion: {
        status: 'pending',
        requested_at: this.iso(this.deletion.requestedAt),
        grace_until: this.iso(this.deletion.graceUntil),
      },
    };
  }

  private route(request: RecordedRequest): Response | null {
    const { method, path, body } = request;
    const override = this.overrides.get(`${method} ${path}`);
    if (override) {
      this.overrides.delete(`${method} ${path}`);
      return override;
    }
    const now = this.auth.nowMs;

    if (method === 'POST' && path === '/v1/auth/reauth') {
      const b = body as { provider?: unknown; id_token?: unknown; nonce?: unknown } | undefined;
      const nonce = typeof b?.nonce === 'string' ? b.nonce : '';
      if (
        (b?.provider !== 'apple' && b?.provider !== 'google') ||
        b.id_token !== 'valid-id-token' ||
        nonce.length < 16 ||
        this.usedNonces.has(nonce)
      ) {
        return problem(401, 'UNAUTHENTICATED');
      }
      this.usedNonces.add(nonce);
      this.counter += 1;
      this.grant = { token: `reauth-${this.counter}`, issuedAt: now };
      return json({ reauth_token: this.grant.token, expires_at: this.iso(now + REAUTH_TTL_MS) });
    }

    if (path === '/v1/me/deletion') {
      if (method === 'GET')
        return this.deletion ? json(this.deletionView()) : problem(404, 'NOT_FOUND');
      if (method === 'DELETE') {
        if (!this.deletion || now >= this.deletion.graceUntil) return problem(404, 'NOT_FOUND');
        this.deletion = null;
        this.onDeletionCancelled?.();
        return new Response(null, { status: 204 });
      }
      if (method === 'POST') {
        if (!this.reauthOk(request)) return problem(401, 'REAUTH_REQUIRED');
        this.deletion ??= { requestedAt: now, graceUntil: now + GRACE_MS };
        this.grant = null;
        this.auth.revokeAll(); // every device, this one included
        return json(this.deletionView(), 202);
      }
    }

    if (method === 'POST' && path === '/v1/me/exports') {
      if (!this.reauthOk(request)) return problem(401, 'REAUTH_REQUIRED');
      const last = this.exports.filter((e) => e.status !== 'cancelled').at(-1);
      if (last && now - last.requestedAt < EXPORT_COOLDOWN_MS) {
        const retry = Math.ceil((last.requestedAt + EXPORT_COOLDOWN_MS - now) / 1000);
        return new Response(JSON.stringify({ status: 429, code: 'EXPORT_COOLDOWN' }), {
          status: 429,
          headers: { 'Content-Type': 'application/problem+json', 'Retry-After': String(retry) },
        });
      }
      this.counter += 1;
      const item: FakeExport = {
        id: `00000000-0000-4000-8000-${String(this.counter).padStart(12, '0')}`,
        status: 'queued',
        requestedAt: now,
        availableAt: now + EXPORT_DELAY_MS,
        expiresAt: null,
      };
      this.exports.push(item);
      return json(this.exportView(item, false), 202);
    }

    const one = /^\/v1\/me\/exports\/([^/]+)$/.exec(path);
    if (one) {
      const item = this.exports.find((e) => e.id === decodeURIComponent(one[1] ?? ''));
      if (method === 'GET') {
        if (!this.reauthOk(request)) return problem(401, 'REAUTH_REQUIRED');
        return item ? json(this.exportView(item, true)) : problem(404, 'NOT_FOUND');
      }
      if (method === 'DELETE') {
        if (!item || item.status !== 'queued') return problem(404, 'NOT_FOUND');
        item.status = 'cancelled';
        return new Response(null, { status: 204 });
      }
    }
    return null;
  }
}
