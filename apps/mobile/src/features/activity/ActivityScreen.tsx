import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef } from 'react';
import { ActivityIndicator, Pressable, StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text, useTheme, useToast } from '../../ui';
import { AuthError } from '../auth';
import { cardQueryKey } from '../feed/queryKeys';
import type { FriendsApi } from '../friends/friendsApi';
import { FRIENDS_QUERY_KEY, REQUESTS_QUERY_KEY } from '../friends/shared';
import { CardsError, type CardsApi } from '../sharing/cardsApi';
import { ActivityError, type ActivityApi, type ActivityItem } from './activityApi';
import { Badge } from './Badge';
import {
  ACTIVITY_QUERY_KEY,
  activityQueryOptions,
  unreadCountOf,
  withNothingUnread,
  type ActivityData,
} from './activityQuery';

export interface ActivityScreenProps {
  activity: ActivityApi;
  cards: CardsApi;
  friends: FriendsApi;
  onOpenCard: (cardId: string) => void;
  onOpenRequests: () => void;
  onOpenFriends: () => void;
  onBack: () => void;
  now?: () => number;
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
/** Card details change rarely; re-reading each one on every visit would spend the read budget. */
const CARD_STALE_MS = 60_000;

const sessionEnded = (error: unknown): boolean =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

function timeLabel(at: number, now: number): string {
  const ago = Math.max(0, now - at);
  if (ago < MINUTE) return t('activity.time.now');
  if (ago < HOUR) return t('activity.time.minutes', { count: Math.floor(ago / MINUTE) });
  if (ago < DAY) return t('activity.time.hours', { count: Math.floor(ago / HOUR) });
  return t('activity.time.days', { count: Math.floor(ago / DAY) });
}

interface RowDeps {
  cards: CardsApi;
  friends: FriendsApi;
}

/** The card an item is about, read through `GET /v1/cards/{id}` (the same cache entry the full-screen view uses). */
function useCard(cards: CardsApi, id: string) {
  return useQuery({
    queryKey: cardQueryKey(id),
    queryFn: () => cards.getCard(id),
    staleTime: CARD_STALE_MS,
  });
}

const cardGone = (error: unknown) => error instanceof CardsError && error.kind === 'not_found';

function useFriendsList(friends: FriendsApi, enabled: boolean) {
  return useQuery({ queryKey: FRIENDS_QUERY_KEY, queryFn: () => friends.listFriends(), enabled });
}

/** One row: the sentence, an optional caption, the time, and a "new" mark; a tap opens what it is about. */
function Row({
  item,
  now,
  text,
  detail,
  onPress,
}: {
  item: ActivityItem;
  now: number;
  text: string;
  detail?: string | null;
  onPress: () => void;
}) {
  const theme = useTheme();
  const when = timeLabel(item.createdAt, now);
  return (
    <Pressable
      testID={`activity-${item.id}`}
      onPress={onPress}
      accessibilityRole="button"
      accessibilityLabel={
        item.unread
          ? t('activity.row.a11y.unread', { text, when })
          : t('activity.row.a11y', { text, when })
      }
      style={[
        styles.row,
        { backgroundColor: theme.color.surface.raised, borderColor: theme.hairline },
      ]}
    >
      <View style={styles.rowText}>
        {item.unread ? <Badge text={t('activity.new')} testID={`activity-new-${item.id}`} /> : null}
        <Text variant="label.md">{text}</Text>
        {detail ? <Text variant="hand.md">{detail}</Text> : null}
        <Text tone="muted">{when}</Text>
      </View>
    </Pressable>
  );
}

function CardRow({
  item,
  now,
  deps,
  onOpenCard,
}: {
  item: ActivityItem;
  now: number;
  deps: RowDeps;
  onOpenCard: (id: string) => void;
}) {
  const card = useCard(deps.cards, item.subjectId);
  const reaction = item.type === 'reaction.new';
  const friendList = useFriendsList(deps.friends, reaction && item.actorId !== null);
  // Unfriend, block or delete: the card read answers 404 and the item leaves the list.
  if (cardGone(card.error)) return null;
  let text: string;
  if (item.type === 'card.new') {
    const name = card.data?.sender.displayName;
    text = name ? t('activity.cardNew', { name }) : t('activity.cardNew.someone');
  } else if (reaction) {
    const name = friendList.data?.find((p) => p.id === item.actorId)?.displayName;
    text = name ? t('activity.reaction', { name }) : t('activity.reaction.someone');
  } else {
    text = t('activity.cardHeld');
  }
  return (
    <Row
      item={item}
      now={now}
      text={text}
      detail={card.data?.caption}
      onPress={() => onOpenCard(item.subjectId)}
    />
  );
}

function RequestRow({
  item,
  now,
  deps,
  onOpenRequests,
}: {
  item: ActivityItem;
  now: number;
  deps: RowDeps;
  onOpenRequests: () => void;
}) {
  const requests = useQuery({
    queryKey: [...REQUESTS_QUERY_KEY, 'incoming'],
    queryFn: () => deps.friends.listRequests('incoming'),
  });
  const request = requests.data?.find((r) => r.id === item.subjectId);
  // Answered, cancelled or expired since: no longer an open request to me.
  if (requests.data && !request) return null;
  return (
    <Row
      item={item}
      now={now}
      text={
        request
          ? t('activity.friendRequest', { name: request.person.displayName })
          : t('activity.friendRequest.someone')
      }
      onPress={onOpenRequests}
    />
  );
}

function AcceptedRow({
  item,
  now,
  deps,
  onOpenFriends,
}: {
  item: ActivityItem;
  now: number;
  deps: RowDeps;
  onOpenFriends: () => void;
}) {
  const friendList = useFriendsList(deps.friends, true);
  const friend = friendList.data?.find((p) => p.id === item.subjectId);
  // Unfriended or blocked since.
  if (friendList.data && !friend) return null;
  return (
    <Row
      item={item}
      now={now}
      text={
        friend
          ? t('activity.friendAccepted', { name: friend.displayName })
          : t('activity.friendAccepted.someone')
      }
      onPress={onOpenFriends}
    />
  );
}

/**
 * The activity tab (T-88, US-31 AC-31.3): every new card, reaction, friend request, accepted request and review
 * hold, also when no push was sent. The server lists ids only and re-checks each item on every read; the app
 * resolves cards through `GET /v1/cards/{id}` and people through the friends routes, and drops an item whose card
 * or person it may no longer see. Viewing the tab marks everything up to the newest item read.
 */
export function ActivityScreen({
  activity,
  cards,
  friends,
  onOpenCard,
  onOpenRequests,
  onOpenFriends,
  onBack,
  now = Date.now,
}: ActivityScreenProps) {
  const theme = useTheme();
  const toast = useToast();
  const queryClient = useQueryClient();
  const list = useInfiniteQuery(activityQueryOptions(activity));
  const items = list.data?.pages.flatMap((page) => page.items) ?? [];
  const newest = items[0];
  const unread = unreadCountOf(list.data) > 0 || items.some((item) => item.unread);
  const markedUpTo = useRef<string | null>(null);

  const markRead = useMutation({
    mutationFn: (upTo: string) => activity.markRead(upTo),
    onSuccess: () =>
      queryClient.setQueryData<ActivityData>(ACTIVITY_QUERY_KEY, (data) => withNothingUnread(data)),
    onError: (error) => {
      if (error instanceof ActivityError && error.kind === 'not_found') {
        // The newest item vanished in between: re-read the list and mark again.
        markedUpTo.current = null;
        void queryClient.invalidateQueries({ queryKey: ACTIVITY_QUERY_KEY });
      }
    },
  });

  const newestId = newest?.id;
  useEffect(() => {
    if (!newestId || !unread || markedUpTo.current === newestId) return;
    markedUpTo.current = newestId;
    markRead.mutate(newestId);
    // `markRead` is stable in behaviour; re-run only when the newest item or the unread state changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [newestId, unread]);

  const loadOlder = () => {
    list.fetchNextPage().then(
      (result) => {
        const error = result.error;
        if (!error || !result.isFetchNextPageError || sessionEnded(error)) return;
        if (error instanceof ActivityError && error.kind === 'invalid') {
          void queryClient.resetQueries({ queryKey: ACTIVITY_QUERY_KEY });
          return;
        }
        toast.show({ message: t('activity.error.older'), variant: 'error' });
      },
      () => undefined,
    );
  };

  const deps: RowDeps = { cards, friends };
  const at = now();
  let body;
  if (list.isPending) {
    body = (
      <ActivityIndicator
        color={theme.color.primary.base}
        accessibilityLabel={t('activity.loading')}
      />
    );
  } else if (list.isError) {
    body = (
      <View style={styles.block} testID="activity-error">
        <Text tone="muted">{t('activity.error.load')}</Text>
        <Button label={t('activity.retry')} onPress={() => void list.refetch()} />
      </View>
    );
  } else if (items.length === 0) {
    body = (
      <Text tone="muted" testID="activity-empty">
        {t('activity.empty')}
      </Text>
    );
  } else {
    body = (
      <View style={styles.block}>
        {items.map((item) => {
          if (item.type === 'friend.request') {
            return (
              <RequestRow
                key={item.id}
                item={item}
                now={at}
                deps={deps}
                onOpenRequests={onOpenRequests}
              />
            );
          }
          if (item.type === 'friend.accepted') {
            return (
              <AcceptedRow
                key={item.id}
                item={item}
                now={at}
                deps={deps}
                onOpenFriends={onOpenFriends}
              />
            );
          }
          return <CardRow key={item.id} item={item} now={at} deps={deps} onOpenCard={onOpenCard} />;
        })}
        {list.hasNextPage ? (
          <Button
            block
            variant="secondary"
            label={t('activity.older')}
            loading={list.isFetchingNextPage}
            onPress={loadOlder}
          />
        ) : null}
      </View>
    );
  }

  return (
    <Screen title={t('activity.title')} testID="activity">
      <Text tone="muted">{t('activity.intro')}</Text>
      {body}
      <Button block variant="ghost" label={t('common.back')} onPress={onBack} />
    </Screen>
  );
}

const styles = StyleSheet.create({
  block: { gap: space[3] },
  row: {
    padding: space[4],
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
  },
  rowText: { gap: space[1] },
});
