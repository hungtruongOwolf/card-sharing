/**
 * In-memory stand-in for the onboarding endpoints on top of {@link FakeAuthServer} (same network-boundary seam).
 *
 * `GET /v1/me` and `POST /v1/me/age` mirror apps/api/src/routes/me.rs and crates/identity/src/age_gate.rs:
 * one submission per account (`AGE_ALREADY_SUBMITTED`), refusal deletes the account and its sessions
 * (`AGE_GATE_REFUSED`, no detail), admission moves to `pending_profile`.
 * `PUT /v1/me/profile` mirrors the display-name route in apps/api/src/routes/me.rs (403 `ONBOARDING_INCOMPLETE`
 * before the age gate, 400 `VALIDATION_FAILED`, 200 `{status, onboarding, profile}`).
 * `PUT /v1/me/handle` mirrors the handle route (lowercases input, 400 `VALIDATION_FAILED`, 409 `HANDLE_TAKEN`;
 * the cooldown is driven by tests through `overrides`). The account becomes active once both a display name and a
 * handle are set.
 */
import { FakeAuthServer, json, problem, type RecordedRequest } from './fakeAuthServer';

type Status = 'pending_age' | 'pending_profile' | 'active';

const STEP: Record<Status, 'age' | 'profile' | 'done'> = {
  pending_age: 'age',
  pending_profile: 'profile',
  active: 'done',
};

export class FakeOnboardingServer {
  readonly auth = new FakeAuthServer();
  status: Status = 'pending_age';
  /** Region threshold the fake gate applies (the client must never learn it). */
  minAge = 13;
  birthDate: string | null = null;
  displayName: string | null = null;
  handle: string | null = null;
  readonly takenHandles = new Set<string>(['taken']);
  /** Next response for a path, consumed once (e.g. a 503 or 429). */
  readonly overrides = new Map<string, Response>();

  constructor() {
    this.auth.routes = (request) => this.route(request);
  }

  get baseUrl(): string {
    return this.auth.baseUrl;
  }

  readonly now = (): number => this.auth.now();

  readonly fetch: typeof fetch = async (input, init) => {
    const response = await this.auth.fetch(input, init);
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    if (url.endsWith('/v1/auth/apple') || url.endsWith('/v1/auth/google')) {
      if (response.ok) this.resetAccount(); // the fake signs every provider token into a fresh account
    }
    return response;
  };

  resetAccount(status: Status = 'pending_age'): void {
    this.status = status;
    this.birthDate = null;
    this.displayName = null;
    this.handle = null;
  }

  count(method: string, path: string): number {
    return this.auth.count(method, path);
  }

  private route({ method, path, body }: RecordedRequest): Response | null {
    const override = this.overrides.get(path);
    if (override) {
      this.overrides.delete(path);
      return override;
    }
    const me = () => ({
      account: { id: 'acc-1', status: this.status },
      onboarding: { step: STEP[this.status] },
      profile: { display_name: this.displayName, handle: this.handle },
      policy: {
        min_age: this.minAge,
        ai_effects: { cloud_allowed: true, people_in_photo: 'any' },
        ads: { rewarded: true, personalized_requires_consent: false },
        ai_label: { visible_badge: true, c2pa: true },
      },
    });

    if (method === 'GET' && path === '/v1/me') return json(me());

    if (method === 'POST' && path === '/v1/me/age') {
      if (this.status !== 'pending_age') return problem(403, 'AGE_ALREADY_SUBMITTED');
      const raw = (body as { birth_date?: unknown } | undefined)?.birth_date;
      if (typeof raw !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(raw)) {
        return problem(400, 'VALIDATION_FAILED');
      }
      const today = new Date(this.auth.nowMs);
      const [y, m, d] = raw.split('-').map(Number) as [number, number, number];
      let age = today.getUTCFullYear() - y;
      if (
        today.getUTCMonth() + 1 < m ||
        (today.getUTCMonth() + 1 === m && today.getUTCDate() < d)
      ) {
        age -= 1;
      }
      if (age < this.minAge) {
        this.auth.dropSessions(); // account deleted with its sessions
        return problem(403, 'AGE_GATE_REFUSED');
      }
      this.birthDate = raw;
      this.status = 'pending_profile';
      return json({ status: this.status, onboarding: { step: 'profile' } });
    }

    if (method === 'PUT' && path === '/v1/me/profile') {
      const name = (body as { display_name?: unknown } | undefined)?.display_name;
      if (this.status === 'pending_age') return problem(403, 'ONBOARDING_INCOMPLETE');
      if (typeof name !== 'string' || name.length < 2 || name.length > 30 || name === 'badword') {
        return problem(400, 'VALIDATION_FAILED');
      }
      this.displayName = name;
      this.activateIfComplete();
      return json({
        status: this.status,
        onboarding: { step: STEP[this.status] },
        profile: { display_name: name, handle: this.handle },
      });
    }

    if (method === 'PUT' && path === '/v1/me/handle') {
      const raw = (body as { handle?: unknown } | undefined)?.handle;
      if (this.status === 'pending_age') return problem(403, 'ONBOARDING_INCOMPLETE');
      const handle = typeof raw === 'string' ? raw.toLowerCase() : null;
      if (handle === null || !/^[a-z0-9._]{3,20}$/.test(handle)) {
        return problem(400, 'VALIDATION_FAILED');
      }
      if (this.takenHandles.has(handle)) return problem(409, 'HANDLE_TAKEN');
      this.handle = handle;
      this.activateIfComplete();
      return json({
        status: this.status,
        onboarding: { step: STEP[this.status] },
        profile: { display_name: this.displayName, handle },
      });
    }
    return null;
  }

  private activateIfComplete() {
    if (this.status === 'pending_profile' && this.displayName && this.handle)
      this.status = 'active';
  }
}
