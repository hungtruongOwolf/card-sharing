/**
 * In-memory stand-in for the notification endpoints (crates/notifications, apps/api/src/routes/
 * notification_preferences.rs, T-87) on top of {@link FakeAuthServer} (same network-boundary seam).
 *
 * It keeps the server's observable rules: defaults until saved (every type on, quiet hours 22:00-08:00, no time
 * zone); `PUT` replaces every field and answers the stored value; strict `HH:MM`, start ≠ end, a known IANA zone
 * and no unknown fields, else 400 `VALIDATION_FAILED`. The caller is the auth fake's only account.
 */
import { FakeAuthServer, json, problem, type RecordedRequest } from './fakeAuthServer';

export interface StoredPrefs {
  types: { card_new: boolean; reaction: boolean; friend: boolean; card_review: boolean };
  quiet_hours: { enabled: boolean; start: string; end: string };
  time_zone: string | null;
}

export const DEFAULT_PREFS: StoredPrefs = {
  types: { card_new: true, reaction: true, friend: true, card_review: true },
  quiet_hours: { enabled: true, start: '22:00', end: '08:00' },
  time_zone: null,
};

/** Zones the fake's "Postgres" knows (`pg_timezone_names`). */
export const KNOWN_ZONES = new Set(['UTC', 'Europe/Paris', 'Asia/Ho_Chi_Minh', 'America/New_York']);

const PREFS_PATH = '/v1/me/notification-preferences';
const CLOCK = /^([01]\d|2[0-3]):[0-5]\d$/;

const exactKeys = (value: unknown, keys: readonly string[]): value is Record<string, unknown> =>
  typeof value === 'object' &&
  value !== null &&
  Object.keys(value).every((k) => keys.includes(k)) &&
  keys.filter((k) => k !== 'time_zone').every((k) => k in value);

function validPrefs(body: unknown): StoredPrefs | null {
  if (!exactKeys(body, ['types', 'quiet_hours', 'time_zone'])) return null;
  const { types, quiet_hours: quiet, time_zone: zone } = body;
  if (!exactKeys(types, ['card_new', 'reaction', 'friend', 'card_review'])) return null;
  if (Object.values(types).some((v) => typeof v !== 'boolean')) return null;
  if (!exactKeys(quiet, ['enabled', 'start', 'end'])) return null;
  if (typeof quiet.enabled !== 'boolean') return null;
  if (typeof quiet.start !== 'string' || !CLOCK.test(quiet.start)) return null;
  if (typeof quiet.end !== 'string' || !CLOCK.test(quiet.end)) return null;
  if (quiet.start === quiet.end) return null;
  if (zone !== undefined && zone !== null && (typeof zone !== 'string' || !KNOWN_ZONES.has(zone))) {
    return null;
  }
  return {
    types: types as StoredPrefs['types'],
    quiet_hours: quiet as StoredPrefs['quiet_hours'],
    time_zone: (zone as string | null | undefined) ?? null,
  };
}

export class FakeNotificationsServer {
  readonly auth = new FakeAuthServer();
  prefs: StoredPrefs = JSON.parse(JSON.stringify(DEFAULT_PREFS)) as StoredPrefs;
  /** Next response for `METHOD path` (path without query), consumed once (e.g. a 503 or 429). */
  readonly overrides = new Map<string, Response>();

  constructor() {
    this.auth.routes = (request) => this.route(request);
  }

  get baseUrl(): string {
    return this.auth.baseUrl;
  }

  readonly fetch: typeof fetch = (input, init) => this.auth.fetch(input, init);
  readonly now = (): number => this.auth.now();

  count(method: string, path: string): number {
    return this.auth.count(method, path);
  }

  get requests(): RecordedRequest[] {
    return this.auth.requests;
  }

  protected route(request: RecordedRequest): Response | null {
    const path = request.path.split('?')[0] ?? request.path;
    const override = this.overrides.get(`${request.method} ${path}`);
    if (override) {
      this.overrides.delete(`${request.method} ${path}`);
      return override;
    }
    if (path === PREFS_PATH && request.method === 'GET') return json(this.prefs);
    if (path === PREFS_PATH && request.method === 'PUT') {
      const next = validPrefs(request.body);
      if (!next) return problem(400, 'VALIDATION_FAILED');
      this.prefs = next;
      return json(this.prefs);
    }
    return null;
  }
}
