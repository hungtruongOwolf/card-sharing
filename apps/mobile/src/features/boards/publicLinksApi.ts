import { problemCode } from '../../data/problemCode';
import type { AuthClient } from '../auth';
import { isPrivateHost } from '../auth/config';
import { isCardId } from '../sharing/cardsApi';
import { boardsFailure, BoardsError } from './boardsApi';

/**
 * Public board links (T-129 contract, api-mvp "Sharing (public links)", US-24). Owner only on the server; the raw
 * URL is returned once at creation and never again (only its hash is stored), so the app shows it once and the
 * list carries no URL. Below the region's `public_links.min_age` the server answers 403 `POLICY_DISALLOWED`.
 */
export const LINK_EXPIRIES = ['24h', '7d', 'never'] as const;
export type LinkExpiry = (typeof LINK_EXPIRIES)[number];

export interface PublicLink {
  readonly id: string;
  /** Epoch milliseconds. */
  readonly createdAt: number;
  /** Epoch milliseconds; null = never expires. */
  readonly expiresAt: number | null;
}

/**
 * A created link. The first answer carries the URL (`<share base>/b/<token>`), shown once. An idempotent replay of
 * the same create (a retry after a lost response) answers the same link without the URL and with
 * `url_already_shown: true` (#333, #357): the raw URL is not kept for replays, so the owner revokes it and creates a
 * new one to share again.
 */
export type CreatedLink = PublicLink &
  (
    | { readonly url: string; readonly urlAlreadyShown: false }
    | { readonly url: null; readonly urlAlreadyShown: true }
  );

/** The owner may not create public links in their region (AC-24.6): the app hides the action from then on. */
export class LinksDisallowedError extends Error {
  override name = 'LinksDisallowedError';
}

export interface PublicLinksApi {
  list(boardId: string): Promise<PublicLink[]>;
  create(boardId: string, expiresIn: LinkExpiry, idempotencyKey: string): Promise<CreatedLink>;
  revoke(linkId: string): Promise<void>;
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);
const time = (v: unknown): number | null => {
  const ms = typeof v === 'string' ? Date.parse(v) : NaN;
  return Number.isFinite(ms) ? ms : null;
};

export function parseLink(value: unknown): PublicLink | null {
  if (!isObject(value) || typeof value.id !== 'string' || !isCardId(value.id)) return null;
  const createdAt = time(value.created_at);
  const expiresAt =
    value.expires_at === null || value.expires_at === undefined ? null : time(value.expires_at);
  if (createdAt === null || (value.expires_at != null && expiresAt === null)) return null;
  return { id: value.id, createdAt, expiresAt };
}

/** `<scheme>://<host>[:port]/b/<token>`: a plain host (no userinfo, path prefix, query or fragment). */
const SHARE_URL =
  /^(https?):\/\/(\[[0-9a-f:]+\]|[a-z0-9.-]+)(?::\d{1,5})?\/b\/[A-Za-z0-9_-]{16,}$/i;

/**
 * Only a share page URL is shown or shared (a hostile body must not inject a scheme or a lookalike host). It must be
 * https, except that development builds accept plain http to a loopback or private-network host: the local stack's
 * share pages run on `http://<pc-lan-ip>:3000` (sprint 10 B3/B4), the same rule as the API base URL (auth/config.ts).
 */
export function parseCreated(value: unknown, allowLocalHttp = false): CreatedLink | null {
  const link = isObject(value) && isObject(value.link) ? value.link : null;
  const parsed = parseLink(link);
  if (!parsed || !link) return null;
  const url = link.url;
  if (url === undefined || url === null) {
    return link.url_already_shown === true ? { ...parsed, url: null, urlAlreadyShown: true } : null;
  }
  const match = typeof url === 'string' ? SHARE_URL.exec(url) : null;
  if (!match || typeof url !== 'string') return null;
  const scheme = (match[1] ?? '').toLowerCase();
  if (scheme !== 'https' && !(allowLocalHttp && isPrivateHost(match[2] ?? ''))) return null;
  return { ...parsed, url, urlAlreadyShown: false };
}

export interface PublicLinksApiOptions {
  /** Accept http share URLs on loopback or private hosts (development builds only, see {@link parseCreated}). */
  allowLocalHttp?: boolean;
}

export function createPublicLinksApi(
  fetcher: AuthClient['fetch'],
  { allowLocalHttp = false }: PublicLinksApiOptions = {},
): PublicLinksApi {
  const id = (value: string) => {
    if (!isCardId(value)) throw new BoardsError('not_found');
    return encodeURIComponent(value);
  };
  return {
    async list(boardId) {
      const response = await fetcher(`/v1/boards/${id(boardId)}/public-links`, { method: 'GET' });
      if (!response.ok) throw await boardsFailure(response);
      const body: unknown = await response.json().catch(() => null);
      if (!isObject(body) || !Array.isArray(body.items)) throw new BoardsError('unavailable');
      return body.items.flatMap((item) => parseLink(item) ?? []);
    },
    async create(boardId, expiresIn, idempotencyKey) {
      const response = await fetcher(`/v1/boards/${id(boardId)}/public-links`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'Idempotency-Key': idempotencyKey },
        body: JSON.stringify({ expires_in: expiresIn }),
      });
      if (response.status === 403 && (await problemCode(response)) === 'POLICY_DISALLOWED') {
        throw new LinksDisallowedError('public links are not allowed for this account');
      }
      if (!response.ok) throw await boardsFailure(response);
      const created = parseCreated(await response.json().catch(() => null), allowLocalHttp);
      if (!created) throw new BoardsError('unavailable');
      return created;
    },
    async revoke(linkId) {
      const response = await fetcher(`/v1/public-links/${id(linkId)}`, { method: 'DELETE' });
      if (!response.ok) throw await boardsFailure(response);
    },
  };
}
