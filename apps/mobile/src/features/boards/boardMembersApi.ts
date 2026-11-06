import { problemCode } from '../../data/problemCode';
import type { AuthClient } from '../auth';
import { isCardId } from '../sharing/cardsApi';
import { boardsFailure, BoardsError } from './boardsApi';

/**
 * Shared-board membership endpoints (T-106/T-107, api-mvp "Boards", ADR-0007 membership by invitation). Membership
 * needs the invitee's acceptance; what a refused invite means is never revealed beyond the server's codes (a block
 * with a member is a silent drop that looks like success). Members a minor rule hides arrive with a display name only.
 */
export interface BoardMember {
  readonly id: string;
  readonly displayName: string | null;
  readonly role: 'owner' | 'collaborator';
  readonly isMe: boolean;
}

/** An invitation of my board as I (the owner) see it: always pending until accepted, cancelled or expired. */
export interface SentInvitation {
  readonly id: string;
  readonly inviteeId: string;
  readonly inviteeName: string | null;
  /** Epoch milliseconds. */
  readonly expiresAt: number;
}

/** An invitation I received. No member list before accepting. */
export interface ReceivedInvitation {
  readonly id: string;
  readonly boardId: string;
  readonly boardTitle: string;
  readonly ownerName: string | null;
  readonly memberCount: number;
  /** Epoch milliseconds. */
  readonly expiresAt: number;
}

export type InviteRefusal = 'already_invited' | 'collaborator_limit';

/** 409 refusals of an invite, by server code. */
export class InviteError extends Error {
  override name = 'InviteError';
  constructor(readonly reason: InviteRefusal) {
    super(`invite refused: ${reason}`);
  }
}

export interface BoardMembersApi {
  members(boardId: string): Promise<BoardMember[]>;
  /** Owner only; `Idempotency-Key` makes a retried invite safe. */
  invite(boardId: string, userId: string, idempotencyKey: string): Promise<void>;
  sentInvitations(boardId: string): Promise<SentInvitation[]>;
  cancelInvitation(invitationId: string): Promise<void>;
  receivedInvitations(): Promise<ReceivedInvitation[]>;
  /** Resolves with the board I joined. */
  accept(invitationId: string): Promise<string>;
  decline(invitationId: string): Promise<void>;
  /** The owner removes a collaborator, or a collaborator passes their own id to leave. */
  removeMember(boardId: string, userId: string): Promise<void>;
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);
const name = (v: unknown): string | null => (typeof v === 'string' && v.trim() ? v : null);
const time = (v: unknown): number | null => {
  const ms = typeof v === 'string' ? Date.parse(v) : NaN;
  return Number.isFinite(ms) ? ms : null;
};
const items = (body: unknown): unknown[] | null =>
  isObject(body) && Array.isArray(body.items) ? body.items : null;

export function parseMember(value: unknown): BoardMember | null {
  if (!isObject(value) || typeof value.id !== 'string' || !isCardId(value.id)) return null;
  return {
    id: value.id,
    displayName: name(value.display_name),
    role: value.role === 'owner' ? 'owner' : 'collaborator',
    isMe: value.is_me === true,
  };
}

function parseSent(value: unknown): SentInvitation | null {
  if (!isObject(value) || typeof value.id !== 'string' || typeof value.invitee_id !== 'string') {
    return null;
  }
  const expiresAt = time(value.expires_at);
  if (expiresAt === null) return null;
  return {
    id: value.id,
    inviteeId: value.invitee_id,
    inviteeName: name(value.invitee_display_name),
    expiresAt,
  };
}

export function parseReceived(value: unknown): ReceivedInvitation | null {
  if (!isObject(value) || typeof value.id !== 'string' || typeof value.board_id !== 'string') {
    return null;
  }
  const expiresAt = time(value.expires_at);
  const count = value.member_count;
  if (expiresAt === null || typeof value.board_title !== 'string' || typeof count !== 'number') {
    return null;
  }
  return {
    id: value.id,
    boardId: value.board_id,
    boardTitle: value.board_title,
    ownerName: name(value.owner_display_name),
    memberCount: count,
    expiresAt,
  };
}

export function createBoardMembersApi(fetcher: AuthClient['fetch']): BoardMembersApi {
  const id = (value: string) => {
    if (!isCardId(value)) throw new BoardsError('not_found');
    return encodeURIComponent(value);
  };
  const send = async (path: string, init: RequestInit): Promise<Response> => {
    const response = await fetcher(path, init);
    if (!response.ok) throw await boardsFailure(response);
    return response;
  };
  const list = async <T>(path: string, parse: (v: unknown) => T | null): Promise<T[]> => {
    const body: unknown = await (await send(path, { method: 'GET' })).json().catch(() => null);
    const raw = items(body);
    if (!raw) throw new BoardsError('unavailable');
    return raw.flatMap((item) => parse(item) ?? []);
  };

  return {
    members: (boardId) => list(`/v1/boards/${id(boardId)}/members`, parseMember),
    async invite(boardId, userId, idempotencyKey) {
      const response = await fetcher(`/v1/boards/${id(boardId)}/invitations`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'Idempotency-Key': idempotencyKey },
        body: JSON.stringify({ user_id: userId }),
      });
      if (response.status === 409) {
        const code = await problemCode(response);
        throw new InviteError(
          code === 'COLLABORATOR_LIMIT_REACHED' ? 'collaborator_limit' : 'already_invited',
        );
      }
      if (!response.ok) throw await boardsFailure(response);
    },
    sentInvitations: (boardId) => list(`/v1/boards/${id(boardId)}/invitations`, parseSent),
    async cancelInvitation(invitationId) {
      await send(`/v1/board-invitations/${id(invitationId)}`, { method: 'DELETE' });
    },
    receivedInvitations: () => list('/v1/me/board-invitations', parseReceived),
    async accept(invitationId) {
      const response = await send(`/v1/board-invitations/${id(invitationId)}/accept`, {
        method: 'POST',
      });
      const body: unknown = await response.json().catch(() => null);
      const boardId = isObject(body) ? body.board_id : null;
      if (typeof boardId !== 'string' || !isCardId(boardId)) throw new BoardsError('unavailable');
      return boardId;
    },
    async decline(invitationId) {
      await send(`/v1/board-invitations/${id(invitationId)}/decline`, { method: 'POST' });
    },
    async removeMember(boardId, userId) {
      await send(`/v1/boards/${id(boardId)}/members/${id(userId)}`, { method: 'DELETE' });
    },
  };
}
