import {
  useInfiniteQuery,
  useMutation,
  useQueryClient,
  type InfiniteData,
} from '@tanstack/react-query';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Image, Pressable, StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text, useTheme, useToast } from '../../ui';
import { AuthError } from '../auth';
import {
  CardsError,
  isRetryable,
  type CardItem,
  type CardsApi,
  type FeedPage,
} from '../sharing/cardsApi';
import type { CachedFeed, CachedFeedItem, FeedCache } from './feedCache';
import { REACTION_DISPLAY, shownReactions } from './reactions';

import { FEED_QUERY_KEY } from './queryKeys';

export { FEED_QUERY_KEY };

export interface FeedScreenProps {
  api: CardsApi;
  cache: FeedCache;
  /** Reaction ids from `/v1/config` (kill switches applied); none are offered without it. */
  reactions?: readonly string[];
  /** Offline outbox (T-78): a reaction that fails for a retryable reason is queued instead of undone. */
  queueReaction?: (cardId: string, reaction: string | null) => Promise<boolean>;
  /** Device clock seam for tests (signed-URL expiries are on the device clock, see `createCardsApi`). */
  now?: () => number;
  /** Opens a card full screen (T-81). */
  onOpen?: (cardId: string) => void;
  onCompose: () => void;
  onBack: () => void;
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
/** Refetch this long before the earliest signed URL expires, so a picture never points at a dead URL. */
const EXPIRY_SKEW_MS = 30_000;
/** At most one URL refresh per this interval: a picture that keeps failing must not loop the feed. */
const URL_REFRESH_MIN_INTERVAL_MS = 60_000;
const CARD_ASPECT = 4 / 3;

type FeedData = InfiniteData<FeedPage, string | null>;

const sessionEnded = (error: unknown): boolean =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

function sentLabel(sentAt: number, now: number): string {
  const ago = Math.max(0, now - sentAt);
  if (ago < MINUTE) return t('feed.sent.now');
  if (ago < HOUR) return t('feed.sent.minutes', { count: Math.floor(ago / MINUTE) });
  if (ago < DAY) return t('feed.sent.hours', { count: Math.floor(ago / HOUR) });
  return t('feed.sent.days', { count: Math.floor(ago / DAY) });
}

const allItems = (data: InfiniteData<FeedPage, unknown> | undefined): CardItem[] =>
  data?.pages.flatMap((page) => page.items) ?? [];

/** The earliest signed-URL expiry on screen, or null when nothing on screen has media. */
const earliestExpiry = (items: readonly CardItem[]): number | null =>
  items.reduce<number | null>(
    (min, item) =>
      item.media && (min === null || item.media.expiresAt < min) ? item.media.expiresAt : min,
    null,
  );

function withReaction(data: FeedData | undefined, cardId: string, kind: string | null) {
  if (!data) return data;
  return {
    ...data,
    pages: data.pages.map((page) => ({
      ...page,
      items: page.items.map((item) => (item.id === cardId ? { ...item, myReaction: kind } : item)),
    })),
  };
}

/**
 * The feed (T-80, US-15): cards friends sent me, newest first, 30 per page with "Show older cards". Signed media URLs
 * are short-lived: the feed refetches just before the earliest one expires and whenever a picture fails to load (a
 * 403 after expiry), and never stores them. The newest items' metadata is kept for offline use (AC-15.1); offline,
 * those items show without pictures. Recipients react privately (US-17: only the sender sees it). Tapping a card
 * opens it full screen (T-81).
 */
export function FeedScreen({
  api,
  cache,
  reactions,
  queueReaction,
  now = Date.now,
  onOpen,
  onCompose,
  onBack,
}: FeedScreenProps) {
  const toast = useToast();
  const queryClient = useQueryClient();
  const [cached, setCached] = useState<CachedFeed | null>(null);
  const feed = useInfiniteQuery({
    queryKey: FEED_QUERY_KEY,
    queryFn: ({ pageParam }) => api.feed(pageParam),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.nextCursor ?? undefined,
  });
  const items = allItems(feed.data);
  const lastUrlRefresh = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    cache.load(now()).then(
      (copy) => !cancelled && setCached(copy),
      () => undefined,
    );
    return () => {
      cancelled = true;
    };
    // Read once when the screen opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cache]);

  // Every successful load replaces the offline copy (dropping cards that are gone, SR-9).
  const { dataUpdatedAt } = feed;
  useEffect(() => {
    if (!feed.data) return;
    cache.save(allItems(feed.data), now()).catch(() => undefined);
    // `dataUpdatedAt` marks a new successful load; `feed.data` is read from that load.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dataUpdatedAt, cache]);

  /** Refetches the whole feed for new signed URLs, at most once a minute (repeated failures do not loop). */
  const refreshUrls = () => {
    const at = now();
    const last = lastUrlRefresh.current;
    if (feed.isFetching || (last !== null && at - last < URL_REFRESH_MIN_INTERVAL_MS)) return;
    lastUrlRefresh.current = at;
    void feed.refetch();
  };

  const expiry = earliestExpiry(items);
  useEffect(() => {
    if (expiry === null) return;
    // Throttled like a failed picture: URLs that are already expired after a refetch do not loop.
    const timer = setTimeout(refreshUrls, Math.max(0, expiry - EXPIRY_SKEW_MS - now()));
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [expiry, dataUpdatedAt]);

  const react = useMutation({
    mutationFn: ({ card, kind }: { card: CardItem; kind: string | null }) =>
      kind ? api.setReaction(card.id, kind) : api.clearReaction(card.id),
    onMutate: async ({ card, kind }) => {
      await queryClient.cancelQueries({ queryKey: FEED_QUERY_KEY });
      queryClient.setQueryData<FeedData>(FEED_QUERY_KEY, (data) =>
        withReaction(data, card.id, kind),
      );
    },
    onError: async (error, { card, kind }) => {
      if (
        queueReaction &&
        isRetryable(error) &&
        (await queueReaction(card.id, kind).catch(() => false))
      ) {
        toast.show({ message: t('feed.reaction.queued') });
        return;
      }
      queryClient.setQueryData<FeedData>(FEED_QUERY_KEY, (data) =>
        withReaction(data, card.id, card.myReaction),
      );
      if (sessionEnded(error)) return;
      const gone = error instanceof CardsError && error.kind === 'not_found';
      toast.show({
        message: t(gone ? 'feed.error.gone' : 'feed.error.react'),
        variant: 'error',
      });
      if (gone) void queryClient.invalidateQueries({ queryKey: FEED_QUERY_KEY });
    },
  });

  const loadOlder = () => {
    feed.fetchNextPage().then(
      (result) => {
        const error = result.error;
        if (!error || !result.isFetchNextPageError || sessionEnded(error)) return;
        if (error instanceof CardsError && error.kind === 'invalid') {
          // A cursor the server no longer accepts: start again from the newest card.
          void queryClient.resetQueries({ queryKey: FEED_QUERY_KEY });
          return;
        }
        toast.show({ message: t('feed.error.older'), variant: 'error' });
      },
      () => undefined,
    );
  };

  const offline = !feed.data && feed.isError;
  const shownReactionIds = shownReactions(reactions);
  const at = now();

  return (
    <Screen title={t('feed.title')} testID="feed">
      {feed.isPending && !cached ? (
        <Text tone="muted" testID="feed-loading">
          {t('feed.loading')}
        </Text>
      ) : null}
      {offline && cached && cached.items.length > 0 ? (
        <View style={styles.block} testID="feed-offline">
          <Text accessibilityRole="alert">{t('feed.offline')}</Text>
          <Button label={t('feed.retry')} onPress={() => void feed.refetch()} />
          {cached.items.map((item) => (
            <CachedRow key={item.id} item={item} now={at} />
          ))}
        </View>
      ) : offline ? (
        <View style={styles.block} testID="feed-error">
          <Text>{t('feed.error.load')}</Text>
          <Button label={t('feed.retry')} onPress={() => void feed.refetch()} />
        </View>
      ) : feed.isPending && cached ? (
        <View style={styles.block}>
          {cached.items.map((item) => (
            <CachedRow key={item.id} item={item} now={at} />
          ))}
        </View>
      ) : null}
      {feed.data && items.length === 0 ? (
        <View style={styles.block} testID="feed-empty">
          <Text>{t('feed.empty')}</Text>
          <Button label={t('feed.compose')} onPress={onCompose} />
        </View>
      ) : null}
      {items.map((item) => (
        <FeedRow
          key={item.id}
          item={item}
          now={at}
          reactions={item.isMine ? [] : shownReactionIds}
          busy={react.isPending && react.variables?.card.id === item.id}
          onImageError={refreshUrls}
          onOpen={onOpen ? () => onOpen(item.id) : undefined}
          onReact={(kind) =>
            react.mutate({ card: item, kind: kind === item.myReaction ? null : kind })
          }
        />
      ))}
      {feed.hasNextPage ? (
        <Button
          block
          variant="secondary"
          label={t('feed.older')}
          loading={feed.isFetchingNextPage}
          onPress={loadOlder}
        />
      ) : null}
      <Button block variant="ghost" label={t('common.back')} onPress={onBack} />
    </Screen>
  );
}

function CardFrame({ children, testID }: { children: ReactNode; testID: string }) {
  const theme = useTheme();
  return (
    <View
      testID={testID}
      style={[
        styles.card,
        { backgroundColor: theme.color.surface.raised, borderColor: theme.hairline },
      ]}
    >
      {children}
    </View>
  );
}

function FeedRow({
  item,
  now,
  reactions,
  busy,
  onImageError,
  onOpen,
  onReact,
}: {
  item: CardItem;
  now: number;
  reactions: readonly string[];
  busy: boolean;
  onImageError: () => void;
  onOpen?: () => void;
  onReact: (kind: string) => void;
}) {
  const theme = useTheme();
  const name = item.sender.displayName ?? t('feed.someone');
  const live = item.media && item.media.expiresAt > now ? item.media : null;
  const label = item.altText ?? t('feed.card.a11y', { name });
  return (
    <CardFrame testID={`feed-card-${item.id}`}>
      <Text variant="label.md">{name}</Text>
      <Pressable
        testID={`feed-open-${item.id}`}
        disabled={!onOpen}
        onPress={onOpen}
        accessible
        accessibilityRole={onOpen ? 'button' : 'image'}
        accessibilityLabel={label}
        accessibilityHint={onOpen ? t('feed.card.openHint') : undefined}
      >
        {live ? (
          <Image
            testID={`feed-image-${item.id}`}
            source={{ uri: live.renderUrl }}
            style={styles.image}
            resizeMode="contain"
            accessibilityIgnoresInvertColors
            onError={onImageError}
          />
        ) : (
          <View
            testID={`feed-placeholder-${item.id}`}
            style={[styles.image, { backgroundColor: theme.color.bg.paperAlt }]}
          />
        )}
      </Pressable>
      {item.caption ? <Text variant="hand.md">{item.caption}</Text> : null}
      <Text tone="muted">{sentLabel(item.sentAt, now)}</Text>
      {reactions.length > 0 ? (
        <View style={styles.reactions} accessibilityLabel={t('feed.reactions.a11y')}>
          {reactions.map((kind) => {
            const display = REACTION_DISPLAY[kind]!;
            const chosen = item.myReaction === kind;
            return (
              <Button
                key={kind}
                label={display.glyph}
                accessibilityLabel={t(display.labelKey)}
                accessibilityHint={t('feed.reactions.hint')}
                accessibilityState={{ selected: chosen, disabled: busy }}
                variant={chosen ? 'primary' : 'ghost'}
                disabled={busy}
                onPress={() => onReact(kind)}
              />
            );
          })}
        </View>
      ) : null}
    </CardFrame>
  );
}

function CachedRow({ item, now }: { item: CachedFeedItem; now: number }) {
  const theme = useTheme();
  const name = item.senderName ?? t('feed.someone');
  return (
    <CardFrame testID={`feed-cached-${item.id}`}>
      <Text variant="label.md">{name}</Text>
      <View
        style={[styles.image, styles.offlineImage, { backgroundColor: theme.color.bg.paperAlt }]}
        accessible
        accessibilityLabel={item.altText ?? t('feed.card.a11y', { name })}
      >
        <Text tone="muted">{t('feed.offline.picture')}</Text>
      </View>
      {item.caption ? <Text variant="hand.md">{item.caption}</Text> : null}
      <Text tone="muted">{sentLabel(item.sentAt, now)}</Text>
    </CardFrame>
  );
}

const styles = StyleSheet.create({
  block: { gap: space[3] },
  card: {
    gap: space[2],
    padding: space[4],
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
  },
  image: { width: '100%', aspectRatio: 1 / CARD_ASPECT, borderRadius: radius.sm },
  offlineImage: { alignItems: 'center', justifyContent: 'center', padding: space[4] },
  reactions: { flexDirection: 'row', flexWrap: 'wrap', gap: space[1] },
});
