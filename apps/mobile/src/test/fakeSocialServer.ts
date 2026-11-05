/**
 * In-memory stand-in for the social endpoints (crates/social, apps/api/src/routes/{invites,friend_requests,
 * user_search,friends,blocks}.rs, T-37..T-47) on top of {@link FakeAuthServer} (same network-boundary seam).
 *
 * It keeps the server's observable rules rather than a happy-path stub: one 404 `NOT_FOUND` for unknown, foreign
 * and malformed ids; `INVITE_EXPIRED` for a reset invite; `Idempotency-Key` required (UUID) and replayed;
 * the 20-friend cap at send and accept (409 `FRIEND_LIMIT_REACHED`); silent blocks (a request between blocked
 * accounts answers the usual 201 and is never delivered); outgoing requests always look pending; exact,
 * case-insensitive handle search with one `{"result":null}` for missing, hidden, blocked and own handles;
 * findable needs `canBeFindable` (403 `POLICY_DISALLOWED`). The caller is the auth fake's only account.
 */
import { FakeAuthServer, json, problem, type RecordedRequest } from './fakeAuthServer';

export interface FakeUser {
  id: string;
  displayName: string;
  handle: string | null;
  findable: boolean;
  /** Incoming requests from this user carry the minor hint (the real server computes it). */
  adultNoMutualFriends: boolean;
}

type RequestStatus = 'pending' | 'dropped' | 'cancelled' | 'accepted' | 'declined';

interface FakeRequest {
  id: string;
  from: string;
  to: string;
  status: RequestStatus;
  createdAt: number;
  expiresAt: number;
}

const FRIEND_MAX = 20;
const REQUEST_TTL_MS = 14 * 24 * 3_600_000;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const noContent = () => new Response(null, { status: 204 });
const notFound = () => problem(404, 'NOT_FOUND');

export const ME = '00000000-0000-4000-8000-00000000000a';
export const SHARE_HOST = 'https://share.test';

/** A valid 22-character invite token derived from `seed` (tests only). */
export const tokenFor = (seed: string): string => seed.padEnd(22, 'x').slice(0, 22);

export class FakeSocialServer {
  readonly auth = new FakeAuthServer();
  readonly users = new Map<string, FakeUser>();
  readonly friends = new Set<string>();
  readonly requests: FakeRequest[] = [];
  readonly blocks = new Map<string, number>();
  /** Live invite tokens of other accounts → owner id; reset ones answer `INVITE_EXPIRED`. */
  readonly invites = new Map<string, string>();
  readonly expiredInvites = new Set<string>();
  myHandle = 'me.cat';
  myInviteToken = tokenFor('mine');
  myInviteExpiresAt: string | null = null;
  findable = true;
  canBeFindable = true;
  /** Next response for `METHOD path` (path without query), consumed once (e.g. a 503 or 429). */
  readonly overrides = new Map<string, Response>();

  private counter = 0;
  private readonly refs = new Map<string, string>();
  private readonly replays = new Map<string, Response>();

  constructor() {
    this.auth.routes = (request) => this.route(request);
  }

  get baseUrl(): string {
    return this.auth.baseUrl;
  }

  readonly fetch: typeof fetch = (input, init) => this.auth.fetch(input, init);
  readonly now = (): number => this.auth.now();

  /** Adds another account; returns it. */
  addUser(displayName: string, extra: Partial<Omit<FakeUser, 'id' | 'displayName'>> = {}) {
    this.counter += 1;
    const user: FakeUser = {
      id: `00000000-0000-4000-8000-${String(this.counter).padStart(12, '0')}`,
      displayName,
      handle: null,
      findable: true,
      adultNoMutualFriends: false,
      ...extra,
    };
    this.users.set(user.id, user);
    return user;
  }

  /** A pending request between the caller and `user` (`incoming` = from them to me). */
  addRequest(user: FakeUser, direction: 'incoming' | 'outgoing'): FakeRequest {
    this.counter += 1;
    const request: FakeRequest = {
      id: `10000000-0000-4000-8000-${String(this.counter).padStart(12, '0')}`,
      from: direction === 'incoming' ? user.id : ME,
      to: direction === 'incoming' ? ME : user.id,
      status: 'pending',
      createdAt: this.now() - this.counter * 60_000,
      expiresAt: this.now() + REQUEST_TTL_MS,
    };
    this.requests.push(request);
    return request;
  }

  count(method: string, pathPrefix: string): number {
    return this.auth.requests.filter((r) => r.method === method && r.path.startsWith(pathPrefix))
      .length;
  }

  private route(request: RecordedRequest): Response | null {
    const { method } = request;
    const [path = '', query = ''] = request.path.split('?');
    const override = this.overrides.get(`${method} ${path}`);
    if (override) {
      this.overrides.delete(`${method} ${path}`);
      return override;
    }
    const segments = path.split('/').filter(Boolean); // ['v1', ...]
    const [, first, second, third] = segments;

    if (first === 'me' && second === 'invite' && segments.length === 3 && method === 'GET') {
      return this.myInvite();
    }
    if (first === 'me' && second === 'invite' && third === 'reset' && method === 'POST') {
      this.expiredInvites.add(this.myInviteToken);
      this.counter += 1;
      this.myInviteToken = tokenFor(`reset${this.counter}`);
      return this.myInvite();
    }
    if (first === 'me' && segments.length === 2 && method === 'GET') {
      return json({ account: { id: ME, status: 'active' }, settings: this.settings() });
    }
    if (first === 'me' && second === 'settings' && method === 'PATCH') {
      return this.updateSettings(request.body);
    }
    if (first === 'invites' && second && method === 'GET') return this.preview(second);
    if (first === 'friend-requests') {
      if (!second && method === 'POST') return this.create(request);
      if (!second && method === 'GET') return this.list(query);
      if (second && !third && method === 'DELETE') return this.answer(second, 'cancel');
      if (second && third === 'accept' && method === 'POST') return this.answer(second, 'accept');
      if (second && third === 'decline' && method === 'POST') return this.answer(second, 'decline');
    }
    if (first === 'users' && second === 'search' && method === 'POST') {
      return this.search(request.body);
    }
    if (first === 'friends' && !second && method === 'GET') {
      return json({ items: [...this.friends].reverse().map((id) => this.summary(id)) });
    }
    if (first === 'friends' && second && method === 'DELETE') {
      return this.friends.delete(second) ? noContent() : notFound();
    }
    if (first === 'blocks' && !second && method === 'POST') return this.block(request.body);
    if (first === 'blocks' && !second && method === 'GET') {
      const items = [...this.blocks].reverse().map(([id, at]) => ({
        user: this.summary(id),
        blocked_at: new Date(at).toISOString(),
      }));
      return json({ items });
    }
    if (first === 'blocks' && second && method === 'DELETE') {
      return this.blocks.delete(second) ? noContent() : notFound();
    }
    return null;
  }

  private summary(id: string) {
    const user = this.users.get(id);
    if (!user) throw new Error(`fake social: unknown user ${id}`);
    return { id: user.id, display_name: user.displayName, handle: user.handle };
  }

  private settings() {
    return {
      findable_by_handle: this.findable && this.canBeFindable,
      can_be_findable: this.canBeFindable,
    };
  }

  private myInvite(): Response {
    return json({
      url: `${SHARE_HOST}/i/${this.myInviteToken}`,
      expires_at: this.myInviteExpiresAt,
    });
  }

  private updateSettings(body: unknown): Response {
    const value = (body as { findable_by_handle?: unknown } | undefined)?.findable_by_handle;
    if (typeof value !== 'boolean') return problem(400, 'VALIDATION_FAILED');
    if (value && !this.canBeFindable) return problem(403, 'POLICY_DISALLOWED');
    this.findable = value;
    return json(this.settings());
  }

  private ownerOf(token: string): string | Response {
    if (this.expiredInvites.has(token)) return problem(404, 'INVITE_EXPIRED');
    if (token === this.myInviteToken) return ME;
    const owner = this.invites.get(token);
    if (!owner || this.blocks.has(owner)) return notFound();
    return owner;
  }

  private preview(token: string): Response {
    const owner = this.ownerOf(token);
    if (owner instanceof Response) return owner;
    const name = owner === ME ? 'Me' : (this.users.get(owner)?.displayName ?? '');
    return json({ inviter: { display_name: name }, is_self: owner === ME });
  }

  private create(request: RecordedRequest): Response {
    const key = request.idempotencyKey;
    if (!key) return problem(400, 'VALIDATION_FAILED');
    if (!UUID.test(key)) return problem(400, 'VALIDATION_FAILED');
    const replay = this.replays.get(key);
    if (replay) return replay.clone();
    const body = (request.body ?? {}) as { invite_token?: unknown; search_ref?: unknown };
    let target: string;
    if (typeof body.invite_token === 'string' && body.search_ref === undefined) {
      const owner = this.ownerOf(body.invite_token);
      if (owner instanceof Response) return owner;
      if (owner === ME) return problem(400, 'VALIDATION_FAILED');
      target = owner;
    } else if (typeof body.search_ref === 'string' && body.invite_token === undefined) {
      const found = this.refs.get(body.search_ref);
      if (!found) return notFound();
      target = found;
    } else {
      return problem(400, 'VALIDATION_FAILED');
    }
    if (this.friends.size >= FRIEND_MAX) return problem(409, 'FRIEND_LIMIT_REACHED');
    if (this.friends.has(target)) return problem(409, 'ALREADY_FRIENDS');
    const user = this.users.get(target);
    if (!user) return notFound();
    const sent = this.addRequest(user, 'outgoing');
    if (this.blocks.has(target)) sent.status = 'dropped';
    const response = json(
      {
        request: {
          id: sent.id,
          status: 'pending',
          expires_at: new Date(sent.expiresAt).toISOString(),
        },
      },
      201,
    );
    this.replays.set(key, response.clone());
    return response;
  }

  private list(query: string): Response {
    const direction = new URLSearchParams(query).get('direction');
    if (direction !== 'incoming' && direction !== 'outgoing') {
      return problem(400, 'VALIDATION_FAILED');
    }
    const items = this.requests
      .filter((r) =>
        direction === 'incoming'
          ? r.to === ME && r.status === 'pending'
          : r.from === ME && (r.status === 'pending' || r.status === 'dropped'),
      )
      .sort((a, b) => b.createdAt - a.createdAt)
      .map((r) => {
        const other = direction === 'incoming' ? r.from : r.to;
        const hint =
          direction === 'incoming' && this.users.get(other)?.adultNoMutualFriends
            ? { hint: 'adult_no_mutual_friends' }
            : {};
        return {
          id: r.id,
          user: this.summary(other),
          created_at: new Date(r.createdAt).toISOString(),
          expires_at: new Date(r.expiresAt).toISOString(),
          ...hint,
        };
      });
    return json({ items });
  }

  private answer(id: string, action: 'accept' | 'decline' | 'cancel'): Response {
    const request = this.requests.find((r) =>
      action === 'cancel'
        ? r.id === id && r.from === ME && (r.status === 'pending' || r.status === 'dropped')
        : r.id === id && r.to === ME && r.status === 'pending',
    );
    if (!request) return notFound();
    if (action === 'cancel') {
      request.status = 'cancelled';
      return noContent();
    }
    if (action === 'decline') {
      request.status = 'declined';
      return noContent();
    }
    if (this.friends.size >= FRIEND_MAX) return problem(409, 'FRIEND_LIMIT_REACHED');
    request.status = 'accepted';
    this.friends.add(request.from);
    return json({ friend: this.summary(request.from) });
  }

  private search(body: unknown): Response {
    const raw = (body as { handle?: unknown } | undefined)?.handle;
    if (typeof raw !== 'string') return problem(400, 'VALIDATION_FAILED');
    const handle = raw.trim().replace(/^@/, '').toLowerCase();
    const user = [...this.users.values()].find((u) => u.handle === handle);
    if (!user || !user.findable || this.blocks.has(user.id) || handle === this.myHandle) {
      return json({ result: null });
    }
    this.counter += 1;
    const ref = `ref${this.counter}`;
    this.refs.set(ref, user.id);
    return json({ result: { ref, display_name: user.displayName } });
  }

  private block(body: unknown): Response {
    const id = (body as { user_id?: unknown } | undefined)?.user_id;
    if (typeof id !== 'string' || !this.users.has(id)) return notFound();
    const related =
      this.friends.has(id) ||
      this.blocks.has(id) ||
      this.requests.some((r) => r.from === id || r.to === id);
    if (!related) return notFound();
    this.friends.delete(id);
    for (const r of this.requests) {
      if (r.from === id && r.to === ME && r.status === 'pending') r.status = 'dropped';
      if (r.from === ME && r.to === id && r.status === 'pending') r.status = 'cancelled';
    }
    if (!this.blocks.has(id)) this.blocks.set(id, this.now());
    return noContent();
  }
}
