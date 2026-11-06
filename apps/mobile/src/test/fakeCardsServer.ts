/**
 * In-memory stand-in for the media and card endpoints (apps/api/src/routes/{media,cards}.rs, T-70..T-84) on top of
 * {@link FakeSocialServer} (same network-boundary seam, same caller).
 *
 * It keeps the server's observable rules rather than a happy-path stub: uploads dedupe on (kind, bytes) with 200
 * vs 201 and refuse a declared type that disagrees with the magic bytes (415); `POST /v1/cards` needs an
 * `Idempotency-Key` equal to `client_card_id`, replays the stored answer for the same body and answers 422
 * `IDEMPOTENCY_KEY_REUSED` for another body; media must be the caller's and of the right kind (one 404); the mask
 * must have the render's exact size (400); unknown style ids are 422; recipients are intersected with current
 * friends; the same `client_card_id` never makes a second card. `GET /v1/cards/{id}` shows status and hold to the
 * owner only and answers one 404 for everything else.
 */
import { registry } from '@card-sharing/effects-registry';

import { json, problem, type RecordedRequest } from './fakeAuthServer';
import { FakeSocialServer, ME } from './fakeSocialServer';

export type FakeCardStatus = 'pending_scan' | 'held_review' | 'active' | 'blocked' | 'failed';

export interface FakeMedia {
  id: string;
  kind: string;
  contentType: string;
  width: number;
  height: number;
  fingerprint: string;
}

export interface FakeCard {
  id: string;
  owner: string;
  clientCardId: string;
  status: FakeCardStatus;
  renderMediaId: string;
  maskMediaId: string | null;
  originalMediaId: string | null;
  style: unknown;
  caption: string | null;
  recipients: string[];
  sentAt: number;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const KINDS = new Set(['card_render', 'mask', 'original', 'avatar']);
const CARD_FIELDS = new Set([
  'client_card_id',
  'render_media_id',
  'mask_media_id',
  'original_media_id',
  'style',
  'caption',
  'alt_text',
  'audience',
]);
const PNG_MAGIC = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const notFound = () => problem(404, 'NOT_FOUND');

/**
 * A minimal byte string with a PNG signature and IHDR size (all the fake reads). `seed` makes different images.
 */
export function fakePng(width: number, height: number, seed = 0): Uint8Array {
  const bytes = new Uint8Array(32);
  bytes.set(PNG_MAGIC, 0);
  bytes.set([0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52], 8);
  const view = new DataView(bytes.buffer);
  view.setUint32(16, width);
  view.setUint32(20, height);
  bytes[31] = seed;
  return bytes;
}

/** A minimal JPEG-looking byte string (SOI marker); the fake takes its size from the arguments. */
export function fakeJpeg(seed = 0): Uint8Array {
  return new Uint8Array([0xff, 0xd8, 0xff, 0xe0, 0, 0, 0, seed]);
}

function sniff(bytes: Uint8Array): { type: string; width: number; height: number } | null {
  if (PNG_MAGIC.every((b, i) => bytes[i] === b) && bytes.length >= 24) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    return { type: 'image/png', width: view.getUint32(16), height: view.getUint32(20) };
  }
  if (bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) {
    return { type: 'image/jpeg', width: 100, height: 100 };
  }
  return null;
}

function styleKnown(style: unknown): boolean {
  if (typeof style !== 'object' || style === null || Array.isArray(style)) return false;
  return Object.entries(style).every(([slot, ref]) => {
    if (slot !== 'cut' && slot !== 'finish') return false;
    const { id, version } = (ref ?? {}) as { id?: unknown; version?: unknown };
    const entry = typeof id === 'string' ? registry.get(id) : undefined;
    return !!entry && entry.version === version;
  });
}

export class FakeCardsServer extends FakeSocialServer {
  readonly media = new Map<string, FakeMedia>();
  readonly cards = new Map<string, FakeCard>();
  /** `METHOD path` whose next request is processed but answered 503, like a response lost on the way back. */
  readonly loseResponse = new Set<string>();
  /** My reaction per card id. */
  readonly reactions = new Map<string, string>();
  /** `reactions` of config/client.yaml (minus kill switches). */
  readonly reactionKinds = new Set([
    'react.heart',
    'react.laugh',
    'react.wow',
    'react.sad',
    'react.fire',
    'react.paw',
    'react.squish',
  ]);
  feedPageSize = 30;
  /** Lifetime of signed URLs in card views. */
  mediaTtlMs = 10 * 60_000;
  /** Answers carry a `Date` header from the server clock, as a real server's do. */
  dateHeader = true;

  private cardCounter = 0;
  private readonly cardReplays = new Map<string, { body: string; response: Response }>();

  constructor() {
    super();
    const social = this.auth.routes;
    this.auth.routes = (request) => this.cardRoute(request) ?? social?.(request) ?? null;
  }

  /** A card sent to me by `from` (another account), already delivered unless a status is given. */
  addIncomingCard(from: string, extra: Partial<FakeCard> = {}): FakeCard {
    this.cardCounter += 1;
    const n = String(this.cardCounter).padStart(12, '0');
    const card: FakeCard = {
      id: `c0000000-0000-4000-8000-${n}`,
      owner: from,
      clientCardId: `d0000000-0000-4000-8000-${n}`,
      status: 'active',
      renderMediaId: `e0000000-0000-4000-8000-${n}`,
      maskMediaId: null,
      originalMediaId: null,
      style: { cut: { id: 'cut.none', version: 1, params: {} } },
      caption: null,
      recipients: [ME],
      sentAt: this.now() - this.cardCounter * 60_000,
      ...extra,
    };
    this.cards.set(card.id, card);
    return card;
  }

  sentCards(): FakeCard[] {
    return [...this.cards.values()].filter((card) => card.owner === ME);
  }

  private cardRoute(request: RecordedRequest): Response | null {
    const { method } = request;
    const [path = '', query = ''] = request.path.split('?');
    const key = `${method} ${path}`;
    const override = this.overrides.get(key);
    const ours = path === '/v1/media' || path === '/v1/feed' || path.startsWith('/v1/cards');
    if (!ours) return null;
    if (override) {
      this.overrides.delete(key);
      return override;
    }
    const response = this.handle(request, method, path, query);
    if (this.loseResponse.delete(key)) return problem(503, 'TEMPORARILY_UNAVAILABLE');
    if (this.dateHeader) response.headers.set('Date', new Date(this.now()).toUTCString());
    return response;
  }

  private handle(request: RecordedRequest, method: string, path: string, query: string) {
    const segments = path.split('/').filter(Boolean); // ['v1', ...]
    const [, first, second, third] = segments;
    if (first === 'media' && !second && method === 'POST') return this.uploadRoute(request, query);
    if (first === 'cards' && !second && method === 'POST') return this.createRoute(request);
    if (first === 'cards' && second && !third && method === 'GET') return this.readRoute(second);
    if (first === 'feed' && !second && method === 'GET') return this.feedRoute(query);
    if (first === 'cards' && second && third === 'reaction' && method === 'PUT') {
      return this.react(second, request.body);
    }
    if (first === 'cards' && second && third === 'reaction' && method === 'DELETE') {
      return this.react(second, null);
    }
    return notFound();
  }

  private uploadRoute(request: RecordedRequest, query: string): Response {
    const kind = new URLSearchParams(query).get('kind') ?? '';
    if (!KINDS.has(kind)) return problem(400, 'VALIDATION_FAILED');
    const bytes = request.rawBody;
    if (!(bytes instanceof Uint8Array) || bytes.length === 0) {
      return problem(400, 'VALIDATION_FAILED');
    }
    const sniffed = sniff(bytes);
    if (!sniffed) return problem(415, 'MEDIA_TYPE_UNSUPPORTED');
    const declared = request.contentType;
    if (declared && declared !== 'application/octet-stream' && declared !== sniffed.type) {
      return problem(415, 'MEDIA_TYPE_UNSUPPORTED');
    }
    const fingerprint = `${kind}:${Array.from(bytes).join(',')}`;
    const existing = [...this.media.values()].find((m) => m.fingerprint === fingerprint);
    if (existing) return json({ media: this.mediaView(existing) }, 200);
    const n = String(this.media.size + 1).padStart(12, '0');
    const media: FakeMedia = {
      id: `a0000000-0000-4000-8000-${n}`,
      kind,
      contentType: sniffed.type,
      width: sniffed.width,
      height: sniffed.height,
      fingerprint,
    };
    this.media.set(media.id, media);
    return json({ media: this.mediaView(media) }, 201);
  }

  private mediaView(media: FakeMedia) {
    return {
      id: media.id,
      kind: media.kind,
      state: 'uploaded',
      content_type: media.contentType,
      byte_size: 1,
      width: media.width,
      height: media.height,
      created_at: new Date(this.now()).toISOString(),
    };
  }

  private createRoute(request: RecordedRequest): Response {
    const key = request.idempotencyKey;
    if (!key || !UUID.test(key)) return problem(400, 'VALIDATION_FAILED');
    const raw = JSON.stringify(request.body ?? null);
    const replay = this.cardReplays.get(key);
    if (replay) {
      return replay.body === raw ? replay.response.clone() : problem(422, 'IDEMPOTENCY_KEY_REUSED');
    }
    const response = this.makeCard(request.body, key);
    if (response.status !== 409 && response.status !== 429 && response.status < 500) {
      this.cardReplays.set(key, { body: raw, response: response.clone() });
    }
    return response;
  }

  private makeCard(raw: unknown, key: string): Response {
    const body = (raw ?? {}) as Record<string, unknown>;
    if (Object.keys(body).some((field) => !CARD_FIELDS.has(field))) {
      return problem(400, 'VALIDATION_FAILED');
    }
    if (body.client_card_id !== key) return problem(400, 'VALIDATION_FAILED');
    const mine = (id: unknown, kind: string) => {
      const media = typeof id === 'string' ? this.media.get(id) : undefined;
      return media && media.kind === kind ? media : null;
    };
    const render = mine(body.render_media_id, 'card_render');
    if (!render) return notFound();
    const mask = body.mask_media_id == null ? null : mine(body.mask_media_id, 'mask');
    if (body.mask_media_id != null && !mask) return notFound();
    if (mask && (mask.width !== render.width || mask.height !== render.height)) {
      return problem(400, 'VALIDATION_FAILED');
    }
    const original =
      body.original_media_id == null ? null : mine(body.original_media_id, 'original');
    if (body.original_media_id != null && !original) return notFound();
    if (!styleKnown(body.style)) return problem(422, 'UNKNOWN_REGISTRY_ID');
    const audience = (body.audience ?? {}) as { mode?: unknown; recipient_ids?: unknown };
    let recipients: string[];
    if (audience.mode === 'all') {
      recipients = [...this.friends];
    } else if (audience.mode === 'subset' && Array.isArray(audience.recipient_ids)) {
      const ids = audience.recipient_ids as unknown[];
      if (ids.length === 0 || new Set(ids).size !== ids.length) {
        return problem(400, 'VALIDATION_FAILED');
      }
      recipients = ids.filter((id): id is string => typeof id === 'string' && this.friends.has(id));
    } else {
      return problem(400, 'VALIDATION_FAILED');
    }
    const existing = [...this.cards.values()].find((card) => card.clientCardId === key);
    const card =
      existing ??
      this.addIncomingCard(ME, {
        clientCardId: key,
        status: 'pending_scan',
        renderMediaId: render.id,
        maskMediaId: mask?.id ?? null,
        originalMediaId: original?.id ?? null,
        style: body.style,
        caption: typeof body.caption === 'string' ? body.caption : null,
        recipients,
        sentAt: this.now(),
      });
    return json(
      { card: { id: card.id, status: 'pending_scan', recipient_count: card.recipients.length } },
      202,
    );
  }

  /** The card read predicate: owner, or a recipient who is still a friend, with no block, of an active card. */
  protected visible(card: FakeCard): boolean {
    if (card.owner === ME) return true;
    return (
      card.status === 'active' &&
      card.recipients.includes(ME) &&
      this.friends.has(card.owner) &&
      !this.blocks.has(card.owner)
    );
  }

  protected cardView(card: FakeCard) {
    const mine = card.owner === ME;
    const delivered = card.status === 'active';
    const sender = mine ? { id: ME, display_name: 'Me' } : this.users.get(card.owner);
    return {
      id: card.id,
      sender: {
        id: card.owner,
        display_name: sender && 'displayName' in sender ? sender.displayName : 'Me',
        avatar_url: null,
      },
      caption: card.caption,
      alt_text: null,
      style: card.style,
      media: delivered
        ? {
            render_url: `https://media.test/${card.renderMediaId}?exp=1&sig=x`,
            mask_url: null,
            original_url: mine && card.originalMediaId ? 'https://media.test/original' : null,
            expires_at: new Date(this.now() + this.mediaTtlMs).toISOString(),
          }
        : null,
      sent_at: new Date(card.sentAt).toISOString(),
      is_mine: mine,
      my_reaction: mine ? null : (this.reactions.get(card.id) ?? null),
      ...(mine
        ? {
            status: card.status,
            hold:
              card.status === 'held_review'
                ? {
                    reason: 'nudity_classifier',
                    appeal: 'none',
                    expires_at: new Date(this.now() + 30 * 86_400_000).toISOString(),
                  }
                : null,
          }
        : {}),
    };
  }

  /** Signed-looking cursor: the offset plus a tag; anything else is a tampered cursor (400). */
  static cursorFor(offset: number): string {
    return `o${offset}t${(offset * 7919 + 13) % 9973}`;
  }

  private feedRoute(query: string): Response {
    const cursor = new URLSearchParams(query).get('cursor');
    let offset = 0;
    if (cursor !== null) {
      const match = /^o(\d+)t(\d+)$/.exec(cursor);
      offset = match ? Number(match[1]) : -1;
      if (offset < 0 || FakeCardsServer.cursorFor(offset) !== cursor) {
        return problem(400, 'VALIDATION_FAILED');
      }
    }
    const visible = [...this.cards.values()]
      .filter((card) => card.owner !== ME && this.visible(card))
      .sort((a, b) => b.sentAt - a.sentAt || b.id.localeCompare(a.id));
    const page = visible.slice(offset, offset + this.feedPageSize);
    const next = offset + page.length;
    return json({
      items: page.map((card) => this.cardView(card)),
      next_cursor: next < visible.length ? FakeCardsServer.cursorFor(next) : null,
    });
  }

  private react(id: string, body: unknown): Response {
    const card = this.cards.get(id);
    if (!card || card.owner === ME || !this.visible(card)) return notFound();
    if (body === null) {
      this.reactions.delete(id);
      return new Response(null, { status: 204 });
    }
    const kind = (body as { kind?: unknown } | undefined)?.kind;
    if (typeof kind !== 'string' || !this.reactionKinds.has(kind)) {
      return problem(400, 'VALIDATION_FAILED');
    }
    this.reactions.set(id, kind);
    return new Response(null, { status: 204 });
  }

  private readRoute(id: string): Response {
    const card = this.cards.get(id);
    if (!card || !this.visible(card)) return notFound();
    return json({ card: this.cardView(card) });
  }
}
