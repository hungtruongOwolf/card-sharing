import { infiniteQueryOptions, type InfiniteData } from '@tanstack/react-query';

import type { ActivityApi, ActivityPage } from './activityApi';

/** Query key of my activity (the tab and the home badge share one cache entry). */
export const ACTIVITY_QUERY_KEY = ['activity'] as const;

export type ActivityData = InfiniteData<ActivityPage, string | null>;

/** The activity list, newest first, one cursor page at a time. */
export const activityQueryOptions = (api: ActivityApi) =>
  infiniteQueryOptions({
    queryKey: ACTIVITY_QUERY_KEY,
    queryFn: ({ pageParam }) => api.page(pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (last: ActivityPage) => last.nextCursor ?? undefined,
  });

/** Unread count of the newest page (the server counts every visible unread item, up to 100). */
export const unreadCountOf = (data: InfiniteData<ActivityPage, unknown> | undefined): number =>
  data?.pages[0]?.unreadCount ?? 0;

/** After a mark-read: nothing counts as unread; the items keep their flags so this visit still shows what was new. */
export const withNothingUnread = (data: ActivityData | undefined): ActivityData | undefined =>
  data && {
    ...data,
    pages: data.pages.map((page) => ({ ...page, unreadCount: 0 })),
  };
