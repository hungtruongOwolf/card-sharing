import {
  type Env,
  type ServiceApiConfig,
  httpUrl,
  readJson,
  serviceGet,
} from '../service-api/service-api.ts';

/**
 * Public board page `/b/<token>` (T-132, US-24, SR-16/SR-17): reads a board snapshot and its OG image
 * through the API's `svc` routes and validates every field before it reaches a page. Server-only.
 * The token never reaches a log; every unusable token is the one neutral `gone`.
 */

/** Public link tokens are 128 random bits, base64url without padding (22 characters). */
const BOARD_TOKEN = /^[A-Za-z0-9_-]{22}$/;

/** CDN cache tag of a link (`pl-<link id, simple>`, api-mvp "Purge"). */
const CACHE_TAG = /^pl-[0-9a-f]{32}$/;

/** Bounds on strings from the API (the API stores at most 480 code points for names and titles). */
const MAX_TEXT = 480;
/** `config/boards.yaml` `max_pins`; anything beyond is dropped. */
const MAX_PINS = 100;
/** Bound on image dimensions from the API (derivatives are at most 1080 px). */
const MAX_DIMENSION = 4096;

/** Same ranges as the API (`crates/boards/src/pins.rs`). */
const SCALE_MIN = 0.1;
const SCALE_MAX = 10;
const ROTATION_MAX = 360;

/**
 * Pin frame in board units over the board size (apps/mobile `PIN_FRAME` 240x320 on a 2400x2400
 * board): a pin at scale 1 is 10% of the board wide and 13.33% high; its content is fitted inside.
 */
export const PIN_FRAME_FRACTION = { width: 240 / 2400, height: 320 / 2400 } as const;

export type PinLayout = {
  /** Centre, as a fraction of the board (0..1). */
  readonly x: number;
  readonly y: number;
  readonly scale: number;
  /** Degrees. */
  readonly rotation: number;
};

export type PublicPin =
  | (PinLayout & {
      readonly kind: 'card';
      readonly imageUrl: string;
      readonly width: number;
      readonly height: number;
    })
  /** Any card guests may not see (a friend's, held, or with an unusable image): position only. */
  | (PinLayout & { readonly kind: 'private_tile' });

export type PublicBoard = {
  readonly title: string;
  readonly ownerDisplayName: string | null;
  /** In z order (back to front). Decorations are not drawn on the web yet. */
  readonly pins: readonly PublicPin[];
  readonly cacheTag: string;
};

export type BoardLookup =
  | { readonly kind: 'found'; readonly board: PublicBoard }
  /** Malformed, unknown, revoked, expired, board or owner gone: one neutral answer. */
  | { readonly kind: 'gone' }
  /** The API could not answer (not configured, down, rate limited). Says nothing about the token. */
  | { readonly kind: 'unavailable' };

export type OgLookup =
  | { readonly kind: 'found'; readonly url: string; readonly cacheTag: string }
  | { readonly kind: 'gone' }
  | { readonly kind: 'unavailable' };

export function isBoardToken(value: string): boolean {
  return BOARD_TOKEN.test(value);
}

/**
 * Origin that serves signed media (the CDN in production): `MEDIA_ORIGIN`, else the origin of
 * `API_URL` (locally the API serves media itself). Image URLs from any other origin are refused,
 * and the page CSP allows images from this origin only.
 */
export function mediaOrigin(env: Env): string | null {
  const configured = env.MEDIA_ORIGIN?.trim() ?? '';
  const url = httpUrl(configured !== '' ? configured : (env.API_URL?.trim() ?? ''));
  return url === null ? null : url.origin;
}

/** Looks the board up. Never throws; a malformed token is answered without calling the API. */
export async function lookupPublicBoard(
  token: string,
  config: ServiceApiConfig,
  clientIp: string,
  media: string | null,
  fetchImpl: typeof fetch = fetch,
): Promise<BoardLookup> {
  if (!isBoardToken(token)) {
    return { kind: 'gone' };
  }
  const response = await serviceGet(`/v1/public/boards/${token}`, config, clientIp, fetchImpl);
  const failure = verdict(response);
  if (failure !== null || response === null) {
    return failure ?? { kind: 'unavailable' };
  }
  const board = parseBoard(await readJson(response), media);
  return board === null ? { kind: 'unavailable' } : { kind: 'found', board };
}

/** Fresh signed OG image URL of the board. Same rules as [`lookupPublicBoard`]. */
export async function lookupPublicBoardOg(
  token: string,
  config: ServiceApiConfig,
  clientIp: string,
  media: string | null,
  fetchImpl: typeof fetch = fetch,
): Promise<OgLookup> {
  if (!isBoardToken(token)) {
    return { kind: 'gone' };
  }
  const response = await serviceGet(`/v1/public/boards/${token}/og`, config, clientIp, fetchImpl);
  const failure = verdict(response);
  if (failure !== null || response === null) {
    return failure ?? { kind: 'unavailable' };
  }
  const body = record(await readJson(response));
  const url = mediaUrl(body?.url, media);
  const cacheTag = body?.cache_tag;
  if (url === null || typeof cacheTag !== 'string' || !CACHE_TAG.test(cacheTag)) {
    return { kind: 'unavailable' };
  }
  return { kind: 'found', url, cacheTag };
}

/** 404 is the neutral `gone`; any other failure is `unavailable`; `null` = go on and parse. */
function verdict(response: Response | null): { kind: 'gone' } | { kind: 'unavailable' } | null {
  if (response === null) return { kind: 'unavailable' };
  if (response.status === 404) return { kind: 'gone' };
  if (response.status !== 200) return { kind: 'unavailable' };
  return null;
}

/** Validates the API body; `null` when it is unusable as a whole. */
export function parseBoard(value: unknown, media: string | null): PublicBoard | null {
  const body = record(value);
  const board = record(body?.board);
  const cacheTag = body?.cache_tag;
  if (board === null || !Array.isArray(body?.pins)) return null;
  if (typeof cacheTag !== 'string' || !CACHE_TAG.test(cacheTag)) return null;
  const title = typeof board.title === 'string' ? board.title.slice(0, MAX_TEXT) : '';
  const owner = board.owner_display_name;
  const ownerDisplayName =
    typeof owner === 'string' && owner.trim() !== '' && owner.length <= MAX_TEXT ? owner : null;
  const pins: PublicPin[] = [];
  for (const raw of body.pins.slice(0, MAX_PINS)) {
    const pin = parsePin(raw, media);
    if (pin !== null) pins.push(pin);
  }
  return { title, ownerDisplayName, pins, cacheTag };
}

function parsePin(value: unknown, media: string | null): PublicPin | null {
  const pin = record(value);
  if (pin === null) return null;
  const x = finite(pin.x);
  const y = finite(pin.y);
  const scale = finite(pin.scale);
  const rotation = finite(pin.rotation);
  if (x === null || y === null || scale === null || rotation === null) return null;
  const layout: PinLayout = {
    x: clamp(x, 0, 1),
    y: clamp(y, 0, 1),
    scale: clamp(scale, SCALE_MIN, SCALE_MAX),
    rotation: clamp(rotation, -ROTATION_MAX, ROTATION_MAX),
  };
  if (pin.kind === 'decoration') return null;
  if (pin.kind === 'card') {
    const imageUrl = mediaUrl(pin.image_url, media);
    const width = dimension(pin.width);
    const height = dimension(pin.height);
    if (imageUrl !== null && width !== null && height !== null) {
      return { kind: 'card', ...layout, imageUrl, width, height };
    }
  }
  // Private tiles, and any card whose image cannot be shown, render the same neutral tile.
  return { kind: 'private_tile', ...layout };
}

/** A signed media URL is accepted only from the media origin, re-serialised. */
function mediaUrl(value: unknown, media: string | null): string | null {
  if (typeof value !== 'string' || media === null) return null;
  const url = httpUrl(value);
  return url !== null && url.origin === media ? url.href : null;
}

function dimension(value: unknown): number | null {
  return typeof value === 'number' && Number.isInteger(value) && value > 0 && value <= MAX_DIMENSION
    ? value
    : null;
}

function finite(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/** Percentages with at most 3 decimals: the stylesheet only ever holds numbers. */
function pct(value: number): string {
  return `${Number((value * 100).toFixed(3))}%`;
}

/**
 * Per-pin placement as a stylesheet (`.pin-<index>`), for a `<style nonce>` element: the nonce CSP
 * blocks `style` attributes. Every value is a validated number, so nothing from the API can inject CSS.
 */
export function pinStylesheet(pins: readonly PinLayout[]): string {
  return pins
    .map((pin, index) => {
      const width = pct(PIN_FRAME_FRACTION.width * pin.scale);
      const height = pct(PIN_FRAME_FRACTION.height * pin.scale);
      const rotation = Number(pin.rotation.toFixed(2));
      return (
        `.pin-${index}{left:${pct(pin.x)};top:${pct(pin.y)};width:${width};height:${height};` +
        `transform:translate(-50%,-50%) rotate(${rotation}deg);z-index:${index + 1}}`
      );
    })
    .join('');
}
