/**
 * In-memory stand-in for the board endpoints (apps/api/src/routes/{boards,board_ops}.rs, T-98/T-99/T-101) on top of
 * {@link FakeAuthServer} (same network-boundary seam). The caller is the auth fake's only account (`ME`).
 *
 * It keeps the server's observable rules: client UUIDv7 board ids (same-owner retry returns the board); members
 * only, everyone else the same 404; pin positions as fractions 0..1; every accepted op bumps the board version and
 * the pin version; `pin.update` / `pin.remove` with a stale `base_version` are `conflict` with the current pin while
 * the rest of the batch still applies; a replayed `op_id` is `applied` again; collaborators change only their own
 * pins (`PIN_NOT_YOURS`); at most 100 live pins; card pins resolve per viewer through `cards` (a card I may not see
 * is a `private_tile`); `since_version` returns changed pins and tombstones plus every live card pin.
 *
 * Membership (T-106/T-107): only the owner invites, and only friends (`friends`), else 404; members plus open
 * invitations stay within 8 (409 `COLLABORATOR_LIMIT_REACHED`); a member or open invitation is 409
 * `ALREADY_INVITED`; accepting needs my pending invitation (else 404); removing a member drops their pins; the
 * owner cannot leave (400).
 */
import { FakeAuthServer, json, problem, type RecordedRequest } from './fakeAuthServer';
import { ME } from './fakeSocialServer';

export interface FakePin {
  id: string;
  createdBy: string;
  cardId: string | null;
  decoration: { id: string; version: number; params: Record<string, unknown> } | null;
  x: number;
  y: number;
  scale: number;
  rotation: number;
  zKey: string;
  version: number;
  changedAt: number;
  removed: boolean;
}

export interface FakeBoard {
  id: string;
  owner: string;
  title: string;
  backdrop: unknown;
  version: number;
  members: Set<string>;
  pins: Map<string, FakePin>;
}

/** A card the fake can resolve: the CardView body and who may see it. */
export interface FakeInvitation {
  id: string;
  boardId: string;
  invitee: string;
  status: 'pending' | 'accepted' | 'declined' | 'cancelled';
}

export interface FakeBoardCard {
  view: Record<string, unknown>;
  visibleTo: Set<string>;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const MAX_PINS = 100;
const notFound = () => problem(404, 'NOT_FOUND');

export class FakeBoardsServer {
  readonly auth = new FakeAuthServer();
  readonly boards = new Map<string, FakeBoard>();
  readonly cards = new Map<string, FakeBoardCard>();
  readonly appliedOps = new Map<string, unknown>();
  /** Next response for `METHOD path` (path without query), consumed once (e.g. a 503). */
  readonly overrides = new Map<string, Response>();
  /** When true every request fails like a dropped connection. */
  offline = false;
  /** My friends (the people I may invite) and every known user's display name. */
  readonly friends = new Set<string>();
  readonly names = new Map<string, string>();
  readonly invitations = new Map<string, FakeInvitation>();
  private nextInvitation = 1;

  constructor() {
    this.auth.routes = (request) => this.route(request);
  }

  get baseUrl(): string {
    return this.auth.baseUrl;
  }

  readonly fetch: typeof fetch = (input, init) =>
    this.offline
      ? Promise.reject(new TypeError('Network request failed'))
      : this.auth.fetch(input, init);
  readonly now = (): number => this.auth.now();

  get requests(): RecordedRequest[] {
    return this.auth.requests;
  }

  count(method: string, path: string): number {
    return this.auth.count(method, path);
  }

  /** Seeds a board owned by `owner` with these members. */
  addBoard(id: string, owner: string, members: string[] = [owner], title = 'Shared'): FakeBoard {
    const board: FakeBoard = {
      id,
      owner,
      title,
      backdrop: null,
      version: 0,
      members: new Set([owner, ...members]),
      pins: new Map(),
    };
    this.boards.set(id, board);
    return board;
  }

  /** Another member's edit, as if made on their phone. */
  editAs(
    boardId: string,
    pinId: string,
    set: Partial<Pick<FakePin, 'x' | 'y' | 'rotation'>>,
  ): void {
    const board = this.boards.get(boardId)!;
    const pin = board.pins.get(pinId)!;
    board.version += 1;
    Object.assign(pin, set, { version: pin.version + 1, changedAt: board.version });
  }

  private view(board: FakeBoard) {
    return {
      id: board.id,
      title: board.title,
      backdrop: board.backdrop,
      role: board.owner === ME ? 'owner' : 'collaborator',
      version: board.version,
      created_at: '2026-11-06T00:00:00Z',
      updated_at: '2026-11-06T00:00:00Z',
    };
  }

  private layout(pin: FakePin) {
    return {
      id: pin.id,
      kind: pin.cardId ? 'card' : 'decoration',
      card_id: pin.createdBy === ME ? pin.cardId : null,
      decoration: pin.decoration,
      x: pin.x,
      y: pin.y,
      scale: pin.scale,
      rotation: pin.rotation,
      z_key: pin.zKey,
      version: pin.version,
      mine: pin.createdBy === ME,
      removed: pin.removed,
    };
  }

  private resolved(pin: FakePin) {
    if (pin.removed) return { id: pin.id, kind: 'removed', version: pin.version };
    const place = {
      id: pin.id,
      x: pin.x,
      y: pin.y,
      scale: pin.scale,
      rotation: pin.rotation,
      z_key: pin.zKey,
      version: pin.version,
      mine: pin.createdBy === ME,
    };
    if (pin.decoration) return { ...place, kind: 'decoration', decoration: pin.decoration };
    const card = pin.cardId ? this.cards.get(pin.cardId) : undefined;
    if (!card) return { id: pin.id, kind: 'removed', version: pin.version };
    return card.visibleTo.has(ME)
      ? { ...place, kind: 'card', card: card.view }
      : { ...place, kind: 'private_tile' };
  }

  protected route(request: RecordedRequest): Response | null {
    const [path = request.path, query = ''] = request.path.split('?');
    const override = this.overrides.get(`${request.method} ${path}`);
    if (override) {
      this.overrides.delete(`${request.method} ${path}`);
      return override;
    }
    if (path === '/v1/boards' && request.method === 'GET') {
      return json({
        items: [...this.boards.values()].filter((b) => b.members.has(ME)).map((b) => this.view(b)),
      });
    }
    if (path === '/v1/boards' && request.method === 'POST') return this.create(request.body);
    const membership = this.membership(request, path);
    if (membership) return membership;
    const match = /^\/v1\/boards\/([^/]+)(\/ops)?$/.exec(path);
    if (!match) return null;
    const board = this.boards.get(match[1] ?? '');
    if (!board || !board.members.has(ME)) return notFound();
    if (!match[2] && request.method === 'GET') {
      const since = new URLSearchParams(query).get('since_version');
      const version = since === null ? null : Number(since);
      const full = version === null || version > board.version;
      const pins = [...board.pins.values()]
        .filter((p) => (full ? !p.removed : p.changedAt > version! || (!p.removed && !!p.cardId)))
        .sort((a, b) => (a.zKey < b.zKey ? -1 : a.zKey > b.zKey ? 1 : 0))
        .map((p) => this.resolved(p));
      return json({ board: this.view(board), pins, full });
    }
    if (match[2] && request.method === 'POST') return this.ops(board, request.body);
    if (!match[2] && request.method === 'PATCH') return this.update(board, request.body);
    return null;
  }

  /** `PATCH /v1/boards/{id}` (T-112): owner only (else 404), at `base_version` (else 409), bumps the version. */
  private update(board: FakeBoard, body: unknown): Response {
    if (board.owner !== ME) return notFound();
    const b = (body ?? {}) as { base_version?: unknown; backdrop?: unknown; title?: unknown };
    if (typeof b.base_version !== 'number' || (!('backdrop' in b) && !('title' in b))) {
      return problem(400, 'VALIDATION_FAILED');
    }
    if (b.base_version !== board.version) return problem(409, 'BOARD_VERSION_CONFLICT');
    if ('backdrop' in b) {
      const id = (b.backdrop as { id?: unknown } | null)?.id;
      if (b.backdrop !== null && (typeof id !== 'string' || !id.startsWith('backdrop.'))) {
        return problem(422, 'UNKNOWN_REGISTRY_ID');
      }
      board.backdrop = b.backdrop;
    }
    if (typeof b.title === 'string') board.title = b.title;
    board.version += 1;
    return json({ board: this.view(board) });
  }

  private membership(request: RecordedRequest, path: string): Response | null {
    if (path === '/v1/me/board-invitations' && request.method === 'GET') {
      return json({
        items: [...this.invitations.values()]
          .filter((i) => i.invitee === ME && i.status === 'pending')
          .map((i) => {
            const board = this.boards.get(i.boardId)!;
            return {
              id: i.id,
              board_id: board.id,
              board_title: board.title,
              owner_display_name: this.names.get(board.owner) ?? null,
              member_count: board.members.size,
              expires_at: '2030-01-01T00:00:00Z',
            };
          }),
      });
    }
    const action = /^\/v1\/board-invitations\/([^/]+)(?:\/(accept|decline))?$/.exec(path);
    if (action) {
      const invitation = this.invitations.get(action[1] ?? '');
      const verb = action[2];
      if (!invitation || invitation.status !== 'pending') return notFound();
      const board = this.boards.get(invitation.boardId)!;
      if (!verb && request.method === 'DELETE') {
        if (board.owner !== ME) return notFound();
        invitation.status = 'cancelled';
        return new Response(null, { status: 204 });
      }
      if (invitation.invitee !== ME || request.method !== 'POST') return notFound();
      if (verb === 'decline') {
        invitation.status = 'declined';
        return new Response(null, { status: 204 });
      }
      invitation.status = 'accepted';
      board.members.add(ME);
      return json({ board_id: board.id });
    }
    const members = /^\/v1\/boards\/([^/]+)\/(members|invitations)(?:\/([^/]+))?$/.exec(path);
    if (!members) return null;
    const board = this.boards.get(members[1] ?? '');
    if (!board || !board.members.has(ME)) return notFound();
    if (members[2] === 'members' && !members[3] && request.method === 'GET') {
      return json({
        items: [board.owner, ...[...board.members].filter((m) => m !== board.owner)].map((id) => ({
          id,
          display_name: this.names.get(id) ?? null,
          handle: null,
          role: id === board.owner ? 'owner' : 'collaborator',
          is_me: id === ME,
          joined_at: '2026-11-06T00:00:00Z',
        })),
      });
    }
    if (members[2] === 'members' && members[3] && request.method === 'DELETE') {
      const target = members[3];
      if (target === board.owner)
        return target === ME ? problem(400, 'VALIDATION_FAILED') : notFound();
      if (!board.members.has(target) || (board.owner !== ME && target !== ME)) return notFound();
      board.members.delete(target);
      for (const pin of board.pins.values()) {
        if (pin.createdBy === target && !pin.removed) {
          board.version += 1;
          Object.assign(pin, { removed: true, version: pin.version + 1, changedAt: board.version });
        }
      }
      return new Response(null, { status: 204 });
    }
    if (board.owner !== ME) return notFound();
    const open = () =>
      [...this.invitations.values()].filter(
        (i) => i.boardId === board.id && i.status === 'pending',
      );
    if (members[2] === 'invitations' && request.method === 'GET') {
      return json({
        items: open().map((i) => ({
          id: i.id,
          invitee_id: i.invitee,
          invitee_display_name: this.names.get(i.invitee) ?? null,
          status: 'pending',
          expires_at: '2030-01-01T00:00:00Z',
        })),
      });
    }
    if (members[2] === 'invitations' && request.method === 'POST') {
      if (!request.idempotencyKey) return problem(400, 'VALIDATION_FAILED');
      const target = (request.body as { user_id?: unknown } | null)?.user_id;
      if (typeof target !== 'string' || !this.friends.has(target)) return notFound();
      if (board.members.has(target) || open().some((i) => i.invitee === target)) {
        return problem(409, 'ALREADY_INVITED');
      }
      if (board.members.size - 1 + open().length >= 8)
        return problem(409, 'COLLABORATOR_LIMIT_REACHED');
      const id = `0192f0ff-0000-7000-8000-${String(this.nextInvitation++).padStart(12, '0')}`;
      this.invitations.set(id, { id, boardId: board.id, invitee: target, status: 'pending' });
      return json(
        {
          invitation: {
            id,
            invitee_id: target,
            status: 'pending',
            expires_at: '2030-01-01T00:00:00Z',
          },
        },
        201,
      );
    }
    return null;
  }

  /** An invitation for me to another owner's board. */
  inviteMe(boardId: string): string {
    const id = `0192f0ff-0000-7000-8000-${String(this.nextInvitation++).padStart(12, '0')}`;
    this.invitations.set(id, { id, boardId, invitee: ME, status: 'pending' });
    return id;
  }

  private create(body: unknown): Response {
    const { id, title, backdrop } = (body ?? {}) as {
      id?: unknown;
      title?: unknown;
      backdrop?: unknown;
    };
    if (typeof id !== 'string' || !UUID.test(id) || typeof title !== 'string' || !title.trim()) {
      return problem(400, 'VALIDATION_FAILED');
    }
    const existing = this.boards.get(id);
    if (existing) return existing.owner === ME ? json({ board: this.view(existing) }) : notFound();
    const board = this.addBoard(id, ME, [ME], title);
    board.backdrop = backdrop ?? null;
    return json({ board: this.view(board) }, 201);
  }

  private ops(board: FakeBoard, body: unknown): Response {
    const ops = (body as { ops?: unknown[] } | null)?.ops;
    if (!Array.isArray(ops) || ops.length === 0 || ops.length > 50)
      return problem(400, 'VALIDATION_FAILED');
    const results = ops.map((raw) => {
      const op = raw as Record<string, unknown>;
      const opId = op.op_id as string;
      const replay = this.appliedOps.get(opId);
      if (replay) return replay;
      const result = this.applyOne(board, op);
      if (result.status === 'applied') this.appliedOps.set(opId, result);
      return result;
    });
    return json({ board_version: board.version, results });
  }

  private applyOne(board: FakeBoard, op: Record<string, unknown>) {
    const opId = op.op_id as string;
    const rejected = (code: string) => ({
      op_id: opId,
      status: 'rejected',
      code,
      pin: null,
      current: null,
    });
    if (op.type === 'pin.add') {
      const p = op.pin as Record<string, unknown>;
      const live = [...board.pins.values()].filter((x) => !x.removed).length;
      if (live >= MAX_PINS) return rejected('PIN_LIMIT_REACHED');
      const cardId = (p.card_id as string | undefined) ?? null;
      if (cardId && !this.cards.get(cardId)?.visibleTo.has(ME)) return rejected('NOT_FOUND');
      board.version += 1;
      const pin: FakePin = {
        id: p.id as string,
        createdBy: ME,
        cardId,
        decoration: (p.decoration as FakePin['decoration']) ?? null,
        x: p.x as number,
        y: p.y as number,
        scale: p.scale as number,
        rotation: p.rotation as number,
        zKey: p.z_key as string,
        version: 1,
        changedAt: board.version,
        removed: false,
      };
      board.pins.set(pin.id, pin);
      return { op_id: opId, status: 'applied', pin: this.layout(pin), current: null, code: null };
    }
    const pin = board.pins.get(op.pin_id as string);
    if (!pin || pin.removed) return rejected('NOT_FOUND');
    if (pin.createdBy !== ME && board.owner !== ME) return rejected('PIN_NOT_YOURS');
    if (op.base_version !== pin.version) {
      return { op_id: opId, status: 'conflict', current: this.layout(pin), pin: null, code: null };
    }
    board.version += 1;
    if (op.type === 'pin.remove') {
      pin.removed = true;
    } else {
      const set = op.set as Record<string, unknown>;
      if (typeof set.x === 'number') pin.x = set.x;
      if (typeof set.y === 'number') pin.y = set.y;
      if (typeof set.scale === 'number') pin.scale = set.scale;
      if (typeof set.rotation === 'number') pin.rotation = set.rotation;
      if (typeof set.z_key === 'string') pin.zKey = set.z_key;
    }
    pin.version += 1;
    pin.changedAt = board.version;
    return { op_id: opId, status: 'applied', pin: this.layout(pin), current: null, code: null };
  }
}
