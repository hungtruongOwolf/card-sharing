import type { InfiniteData, QueryClient } from '@tanstack/react-query';

import type { CardsApi, FeedPage } from '../sharing/cardsApi';
import type { FeedCache } from './feedCache';
import { cardQueryKey, FEED_QUERY_KEY } from './queryKeys';

/** While the app is in the foreground, cached cards are checked this often (T-83: purges reach clients ≤ 60 s). */
export const PURGE_CHECK_INTERVAL_MS = 30_000;

export interface PurgeSyncDeps {
  api: CardsApi;
  cache: FeedCache;
  queryClient: QueryClient;
  now?: () => number;
}

/**
 * Applies the server's purge instructions (T-83, SR-9) to every copy of friends' cards on the device: the offline
 * feed copy (SecureStore) and the in-memory feed and card queries. The server answers from the card-read predicate,
 * so unfriend, block (either way) and deleted cards or senders are all listed. Returns the purged ids. A failed
 * check (offline, rate limit, signed out) changes nothing and is retried on the next trigger.
 *
 * Best effort by nature (threat-model A-03): a patched client can ignore it, which is why every read is checked on
 * the server and signed URLs are short-lived.
 */
export async function syncPurges({
  api,
  cache,
  queryClient,
  now = Date.now,
}: PurgeSyncDeps): Promise<string[]> {
  const feed = queryClient.getQueryData<InfiniteData<FeedPage>>(FEED_QUERY_KEY);
  const ids = new Set<string>();
  for (const page of feed?.pages ?? []) for (const item of page.items) ids.add(item.id);
  for (const item of (await cache.load(now()))?.items ?? []) ids.add(item.id);
  if (ids.size === 0) return [];

  const purged = await api.checkPurges([...ids]);
  if (purged.length === 0) return [];
  const gone = new Set(purged);
  await cache.remove(purged);
  queryClient.setQueryData<InfiniteData<FeedPage>>(FEED_QUERY_KEY, (data) =>
    data
      ? {
          ...data,
          pages: data.pages.map((page) => ({
            ...page,
            items: page.items.filter((item) => !gone.has(item.id)),
          })),
        }
      : data,
  );
  // An open full-screen card re-reads, gets 404 and closes with "card removed".
  for (const id of purged) void queryClient.invalidateQueries({ queryKey: cardQueryKey(id) });
  return purged;
}
