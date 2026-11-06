import { isZKey } from '@card-sharing/canvas-engine';

import { problemCode } from '../../data/problemCode';
import type { AuthClient } from '../auth';
import { clockSkew, isCardId, onDeviceClock, parseCard } from '../sharing/cardsApi';
import { BOARD_SIZE, type BoardPin, type RegistryRef } from './boardModel';
import type { PinOp } from './boardOps';

/**
 * Board endpoints (T-98/T-99/T-101, api-mvp "Boards"). Pin positions travel as fractions of the board (0..1) and are
 * board units in the app (`BOARD_SIZE`). Every answer is validated here; a pin whose card the caller may not see
 * arrives as `private_tile` with no card data, and the app never asks for more.
 */

export type BoardsErrorKind =
  /** 404 `NOT_FOUND`: no board of which I am a member (missing, deleted, or I was removed). */
  | 'not_found'
  /** 400 `VALIDATION_FAILED`. */
  | 'invalid'
  /** 422 `UNKNOWN_REGISTRY_ID`: a backdrop or decoration the server does not know. */
  | 'unknown_registry_id'
  /** 409 `BOARD_LIMIT_REACHED` (AC-18.2). */
  | 'limit'
  /** 403 (onboarding, suspension). */
  | 'not_allowed'
  | 'rate_limited'
  /** Network, 5xx, unexpected status or body: retry later. */
  | 'unavailable';

export class BoardsError extends Error {
  override name = 'BoardsError';
  constructor(
    readonly kind: BoardsErrorKind,
    readonly status?: number,
  ) {
    super(status === undefined ? `boards: ${kind}` : `boards: ${kind} (${status})`);
  }
}

export interface BoardMeta {
  readonly id: string;
  readonly title: string;
  readonly backdrop: RegistryRef | null;
  readonly role: 'owner' | 'collaborator';
  readonly version: number;
}

/** A pin from `GET /v1/boards/{id}`: a drawable pin, or the id of a pin to drop. */
export type RemotePin =
  | { readonly kind: 'pin'; readonly pin: BoardPin; readonly mine: boolean }
  | { readonly kind: 'removed'; readonly id: string };

export interface BoardDetail {
  readonly board: BoardMeta;
  readonly pins: readonly RemotePin[];
  /** True: a snapshot replacing every local pin; false: a delta to merge. */
  readonly full: boolean;
}

/** A pin's placement in an op result (no card contents, by design). */
export interface PinLayout {
  readonly id: string;
  readonly x: number;
  readonly y: number;
  readonly scale: number;
  readonly rotation: number;
  readonly zKey: string;
  readonly version: number;
  readonly removed: boolean;
}

export type OpResult =
  | { readonly opId: string; readonly status: 'applied'; readonly pin: PinLayout | null }
  | { readonly opId: string; readonly status: 'conflict'; readonly current: PinLayout | null }
  | { readonly opId: string; readonly status: 'rejected'; readonly code: string };

export interface OpsResponse {
  readonly boardVersion: number;
  readonly results: readonly OpResult[];
}

export interface BoardsApi {
  list(): Promise<BoardMeta[]>;
  create(board: { id: string; title: string; backdrop: RegistryRef | null }): Promise<BoardMeta>;
  get(id: string, sinceVersion?: number): Promise<BoardDetail>;
  /** 1..{@link MAX_OPS_PER_BATCH} ops with distinct op ids, applied in order. */
  ops(id: string, ops: readonly PinOp[]): Promise<OpsResponse>;
}

/** `POST /v1/boards/{id}/ops` accepts at most this many ops. */
export const MAX_OPS_PER_BATCH = 50;

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);
const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);

const toServerX = (x: number) => x / BOARD_SIZE.width;
const toServerY = (y: number) => y / BOARD_SIZE.height;
const fromServerX = (x: number) => x * BOARD_SIZE.width;
const fromServerY = (y: number) => y * BOARD_SIZE.height;

function registryRef(value: unknown): RegistryRef | null {
  if (!isObject(value) || typeof value.id !== 'string') return null;
  const version = num(value.version);
  if (version === null) return null;
  return { id: value.id, version, params: isObject(value.params) ? value.params : {} };
}

export function parseBoardMeta(value: unknown): BoardMeta | null {
  if (!isObject(value)) return null;
  const { id, title, role } = value;
  const version = num(value.version);
  if (typeof id !== 'string' || !isCardId(id) || typeof title !== 'string' || version === null) {
    return null;
  }
  return {
    id,
    title,
    backdrop: registryRef(value.backdrop),
    role: role === 'collaborator' ? 'collaborator' : 'owner',
    version,
  };
}

function placement(value: Record<string, unknown>) {
  const x = num(value.x);
  const y = num(value.y);
  const scale = num(value.scale);
  const rotation = num(value.rotation);
  const version = num(value.version);
  const zKey = value.z_key;
  if (x === null || y === null || scale === null || rotation === null || version === null) {
    return null;
  }
  if (typeof zKey !== 'string' || !isZKey(zKey)) return null;
  return { x: fromServerX(x), y: fromServerY(y), scale, rotation, zKey, version };
}

export function parseBoardPin(value: unknown, skew: number): RemotePin | null {
  if (!isObject(value) || typeof value.id !== 'string' || !isCardId(value.id)) return null;
  const { id, kind } = value;
  if (kind === 'removed') return { kind: 'removed', id };
  const at = placement(value);
  if (!at) return null;
  const mine = value.mine === true;
  if (kind === 'card') {
    const card = parseCard(value.card);
    if (!card) return null;
    return {
      kind: 'pin',
      mine,
      pin: { id, ...at, content: { kind: 'card', card: onDeviceClock(card, skew) } },
    };
  }
  if (kind === 'decoration') {
    const decoration = registryRef(value.decoration);
    if (!decoration) return null;
    return { kind: 'pin', mine, pin: { id, ...at, content: { kind: 'decoration', decoration } } };
  }
  if (kind === 'private_tile') {
    return { kind: 'pin', mine, pin: { id, ...at, content: { kind: 'private_tile' } } };
  }
  return null;
}

export function parseBoardDetail(body: unknown, skew: number): BoardDetail | null {
  if (!isObject(body) || !Array.isArray(body.pins) || typeof body.full !== 'boolean') return null;
  const board = parseBoardMeta(body.board);
  if (!board) return null;
  // An unreadable pin is skipped, never the whole board.
  const pins = body.pins.flatMap((raw) => parseBoardPin(raw, skew) ?? []);
  return { board, pins, full: body.full };
}

function parseLayout(value: unknown): PinLayout | null {
  if (!isObject(value) || typeof value.id !== 'string') return null;
  const at = placement(value);
  if (!at) return null;
  return { id: value.id, ...at, removed: value.removed === true };
}

export function parseOpsResponse(body: unknown): OpsResponse | null {
  if (!isObject(body) || !Array.isArray(body.results)) return null;
  const boardVersion = num(body.board_version);
  if (boardVersion === null) return null;
  const results: OpResult[] = [];
  for (const raw of body.results) {
    if (!isObject(raw) || typeof raw.op_id !== 'string') return null;
    const opId = raw.op_id;
    if (raw.status === 'applied')
      results.push({ opId, status: 'applied', pin: parseLayout(raw.pin) });
    else if (raw.status === 'conflict') {
      results.push({ opId, status: 'conflict', current: parseLayout(raw.current) });
    } else if (raw.status === 'rejected') {
      results.push({
        opId,
        status: 'rejected',
        code: typeof raw.code === 'string' ? raw.code : 'UNKNOWN',
      });
    } else return null;
  }
  return { boardVersion, results };
}

/** The wire form of an op: fractions for x/y, snake_case, card id only (never card contents). */
export function opBody(op: PinOp): Record<string, unknown> {
  switch (op.type) {
    case 'pin.add': {
      const { pin } = op;
      return {
        type: 'pin.add',
        op_id: op.opId,
        pin: {
          id: pin.id,
          ...('card' in pin
            ? { card_id: pin.card.id }
            : {
                decoration: {
                  id: pin.decoration.id,
                  version: pin.decoration.version,
                  params: pin.decoration.params,
                },
              }),
          x: toServerX(pin.x),
          y: toServerY(pin.y),
          scale: pin.scale,
          rotation: pin.rotation,
          z_key: pin.zKey,
        },
      };
    }
    case 'pin.update': {
      const set: Record<string, unknown> = {};
      if (op.set.x !== undefined) set.x = toServerX(op.set.x);
      if (op.set.y !== undefined) set.y = toServerY(op.set.y);
      if (op.set.scale !== undefined) set.scale = op.set.scale;
      if (op.set.rotation !== undefined) set.rotation = op.set.rotation;
      if (op.set.zKey !== undefined) set.z_key = op.set.zKey;
      return {
        type: 'pin.update',
        op_id: op.opId,
        pin_id: op.pinId,
        base_version: op.baseVersion,
        set,
      };
    }
    case 'pin.remove':
      return { type: 'pin.remove', op_id: op.opId, pin_id: op.pinId, base_version: op.baseVersion };
  }
}

export async function boardsFailure(response: Response): Promise<BoardsError> {
  const { status } = response;
  const code = await problemCode(response);
  if (code === 'UNKNOWN_REGISTRY_ID') return new BoardsError('unknown_registry_id', status);
  if (code === 'BOARD_LIMIT_REACHED') return new BoardsError('limit', status);
  if (status === 404) return new BoardsError('not_found', status);
  if (status === 400) return new BoardsError('invalid', status);
  if (status === 403) return new BoardsError('not_allowed', status);
  if (status === 429) return new BoardsError('rate_limited', status);
  return new BoardsError('unavailable', status);
}

/**
 * Board calls over the authorized `fetch` of the auth client. A revoked session surfaces as `AuthError` from the
 * fetcher; everything else is a {@link BoardsError}. Ids reach a URL only after validation. Signed-URL expiries in
 * card pins are moved onto the device clock like the feed's.
 */
export function createBoardsApi(
  fetcher: AuthClient['fetch'],
  { now = Date.now }: { now?: () => number } = {},
): BoardsApi {
  const read = async <T>(
    parse: (body: unknown, skew: number) => T | null,
    path: string,
    init: RequestInit,
  ): Promise<T> => {
    const response = await fetcher(path, init);
    if (!response.ok) throw await boardsFailure(response);
    const skew = clockSkew(response, now());
    const parsed = parse(await response.json().catch(() => undefined), skew);
    if (parsed === null) throw new BoardsError('unavailable', response.status);
    return parsed;
  };
  const checkedId = (id: string): string => {
    if (!isCardId(id)) throw new BoardsError('not_found');
    return encodeURIComponent(id);
  };
  const json = (body: unknown): RequestInit => ({
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });

  return {
    list: () =>
      read(
        (body) =>
          isObject(body) && Array.isArray(body.items)
            ? body.items.flatMap((item) => parseBoardMeta(item) ?? [])
            : null,
        '/v1/boards',
        { method: 'GET' },
      ),
    create: (board) =>
      read(
        (body) => (isObject(body) ? parseBoardMeta(body.board) : null),
        '/v1/boards',
        json({ id: board.id, title: board.title, backdrop: board.backdrop }),
      ),
    get: (id, sinceVersion) => {
      const query =
        sinceVersion !== undefined && Number.isSafeInteger(sinceVersion) && sinceVersion >= 0
          ? `?since_version=${sinceVersion}`
          : '';
      return read(parseBoardDetail, `/v1/boards/${checkedId(id)}${query}`, { method: 'GET' });
    },
    ops: (id, ops) => {
      if (ops.length === 0 || ops.length > MAX_OPS_PER_BATCH) {
        return Promise.reject(new BoardsError('invalid'));
      }
      return read(
        (body) => parseOpsResponse(body),
        `/v1/boards/${checkedId(id)}/ops`,
        json({ ops: ops.map(opBody) }),
      );
    },
  };
}
