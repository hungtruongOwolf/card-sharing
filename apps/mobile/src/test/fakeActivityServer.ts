/**
 * In-memory stand-in for the activity endpoints (crates/notifications/src/activity.rs, apps/api/src/routes/
 * activity.rs, T-88) on top of {@link FakeCardsServer} (same network-boundary seam, same caller).
 *
 * It keeps the server's observable rules: newest first, ids only; every item is re-checked on each read with the
 * owning module's predicate (`card.new` = the card read predicate, `reaction.new` = my undeleted card and still
 * friends with the reactor, `card.held` = my undeleted card, `friend.request` = still an open request to me,
 * `friend.accepted` = still friends without a block), so unfriend, block, delete and answered requests drop items
 * and never count as unread; `unread_count` stops at 100; a signed cursor (tampered = 400); `POST /v1/activity/read
 * {up_to}` moves a watermark that never goes back, and an id that is not one of my visible items is 404.
 */
import { json, problem, type RecordedRequest } from './fakeAuthServer';
import { FakeCardsServer } from './fakeCardsServer';
import { ME } from './fakeSocialServer';

export type FakeActivityType =
  'card.new' | 'reaction.new' | 'friend.request' | 'friend.accepted' | 'card.held';

export interface FakeActivity {
  id: string;
  type: FakeActivityType;
  subjectId: string;
  actorId: string | null;
  createdAt: number;
  /** Insertion order; the watermark compares on it. */
  seq: number;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const UNREAD_CAP = 100;

export class FakeActivityServer extends FakeCardsServer {
  readonly activity: FakeActivity[] = [];
  activityPageSize = 30;
  /** Sequence of the newest item marked read (0 = nothing read). */
  readWatermark = 0;
  private activitySeq = 0;

  constructor() {
    super();
    const inner = this.auth.routes;
    this.auth.routes = (request) => this.activityRoute(request) ?? inner?.(request) ?? null;
  }

  /** Records an item as the outbox subscriber would; newer than every earlier one. */
  addActivity(type: FakeActivityType, subjectId: string, actorId: string | null = null) {
    this.activitySeq += 1;
    const item: FakeActivity = {
      id: `a0000000-0000-4000-8000-${String(this.activitySeq).padStart(12, '0')}`,
      type,
      subjectId,
      actorId,
      createdAt: this.now() - 1000 + this.activitySeq,
      seq: this.activitySeq,
    };
    this.activity.push(item);
    return item;
  }

  private stillVisible(item: FakeActivity): boolean {
    switch (item.type) {
      case 'card.new': {
        const card = this.cards.get(item.subjectId);
        return !!card && card.owner !== ME && this.visible(card);
      }
      case 'reaction.new': {
        const card = this.cards.get(item.subjectId);
        return (
          !!card &&
          card.owner === ME &&
          item.actorId !== null &&
          this.friends.has(item.actorId) &&
          !this.blocks.has(item.actorId)
        );
      }
      case 'card.held': {
        const card = this.cards.get(item.subjectId);
        return !!card && card.owner === ME;
      }
      case 'friend.request': {
        const request = this.requests.find((r) => r.id === item.subjectId);
        return (
          !!request &&
          request.to === ME &&
          request.status === 'pending' &&
          request.expiresAt > this.now() &&
          !this.blocks.has(request.from)
        );
      }
      case 'friend.accepted':
        return this.friends.has(item.subjectId) && !this.blocks.has(item.subjectId);
    }
  }

  private visibleActivity(): FakeActivity[] {
    return this.activity.filter((item) => this.stillVisible(item)).sort((a, b) => b.seq - a.seq);
  }

  static activityCursor(offset: number): string {
    return `a${offset}t${(offset * 7919 + 17) % 9973}`;
  }

  private activityRoute(request: RecordedRequest): Response | null {
    const [path = '', query = ''] = request.path.split('?');
    if (path !== '/v1/activity' && path !== '/v1/activity/read') return null;
    const key = `${request.method} ${path}`;
    const override = this.overrides.get(key);
    if (override) {
      this.overrides.delete(key);
      return override;
    }
    if (path === '/v1/activity' && request.method === 'GET') return this.listActivity(query);
    if (path === '/v1/activity/read' && request.method === 'POST')
      return this.markRead(request.body);
    return problem(404, 'NOT_FOUND');
  }

  private listActivity(query: string): Response {
    const cursor = new URLSearchParams(query).get('cursor');
    let offset = 0;
    if (cursor !== null) {
      const match = /^a(\d+)t(\d+)$/.exec(cursor);
      offset = match ? Number(match[1]) : -1;
      if (offset < 0 || FakeActivityServer.activityCursor(offset) !== cursor) {
        return problem(400, 'VALIDATION_FAILED');
      }
    }
    const visible = this.visibleActivity();
    const page = visible.slice(offset, offset + this.activityPageSize);
    const next = offset + page.length;
    return json({
      items: page.map((item) => ({
        id: item.id,
        type: item.type,
        subject_id: item.subjectId,
        actor_id: item.actorId,
        created_at: new Date(item.createdAt).toISOString(),
        unread: item.seq > this.readWatermark,
      })),
      unread_count: Math.min(
        visible.filter((item) => item.seq > this.readWatermark).length,
        UNREAD_CAP,
      ),
      next_cursor: next < visible.length ? FakeActivityServer.activityCursor(next) : null,
    });
  }

  private markRead(body: unknown): Response {
    const fields = typeof body === 'object' && body !== null ? Object.keys(body) : [];
    const upTo = (body as { up_to?: unknown } | undefined)?.up_to;
    if (fields.length !== 1 || typeof upTo !== 'string' || !UUID.test(upTo)) {
      return problem(400, 'VALIDATION_FAILED');
    }
    const item = this.visibleActivity().find((i) => i.id === upTo);
    if (!item) return problem(404, 'NOT_FOUND');
    this.readWatermark = Math.max(this.readWatermark, item.seq);
    return new Response(null, { status: 204 });
  }
}
