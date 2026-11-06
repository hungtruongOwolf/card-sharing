/** Query keys of the feed screens (kept apart so non-UI modules such as `purges.ts` need no screen imports). */
export const FEED_QUERY_KEY = ['feed'] as const;
export const cardQueryKey = (id: string) => ['card', id] as const;
