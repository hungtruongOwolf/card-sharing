import { useInfiniteQuery, useQueryClient, type InfiniteData } from '@tanstack/react-query';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Image, Pressable, StyleSheet, View } from 'react-native';

import { linearGradient, radius, size, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Avatar, Button, IconButton, Screen, Text, useTheme, useToast } from '../../ui';
import { AiBadge } from '../ai-effects/AiBadge';
import { AuthError } from '../auth';
import { CardsError, type CardItem, type CardsApi, type FeedPage } from '../sharing/cardsApi';
import type { CachedFeed, CachedFeedItem, FeedCache } from './feedCache';
import { FEED_QUERY_KEY } from './queryKeys';
import { ReactionBar } from './ReactionBar';
import { shownReactions } from './reactions';
import type { SeenMarks } from './seenCards';
import { storyRings } from './stories';
import { StoryRings } from './StoryRings';
import { useCardReaction } from './useCardReaction';

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
  /** Opens a card full screen (T-81): the story viewer. Without it there are no story rings either. */
  onOpen?: (cardId: string) => void;
  /** Which friends' cards I already opened (story rings); none = everything is new. */
  seen?: SeenMarks;
  /** Replaces the default header (title + back), e.g. the home screen's top bar. */
  header?: ReactNode;
  onCompose: () => void;
  /** Shows a back button in the default header. */
  onBack?: () => void;
  /** Empty space after the last card (room for a floating button). */
  bottomInset?: number;
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
/** Refetch this long before the earliest signed URL expires, so a picture never points at a dead URL. */
const EXPIRY_SKEW_MS = 30_000;
/** At most one URL refresh per this interval: a picture that keeps failing must not loop the feed. */
const URL_REFRESH_MIN_INTERVAL_MS = 60_000;
const CARD_ASPECT = 4 / 3;
const NO_MARKS: SeenMarks = {};

const sessionEnded = (error: unknown): boolean =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

/** "Just now", "5 minutes ago"... (shared with the story viewer). */
export function sentLabel(sentAt: number, now: number): string {
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

/**
 * The feed (T-80, US-15; Stories style since sprint 9): a row of story rings (friends with new cards ringed in the
 * story gradient), then the cards as full-width tiles with the sender and caption over the picture. 30 per page with
 * "Show older cards". Signed media URLs are short-lived: the feed refetches just before the earliest one expires and
 * whenever a picture fails to load (a 403 after expiry), and never stores them. The newest items' metadata is kept for
 * offline use (AC-15.1); offline, those items show without pictures. Recipients react privately (US-17: only the
 * sender sees it). Tapping a card or a ring opens the story viewer (T-81).
 */
export function FeedScreen({
  api,
  cache,
  reactions,
  queueReaction,
  now = Date.now,
  onOpen,
  seen = NO_MARKS,
  header,
  onCompose,
  onBack,
  bottomInset = 0,
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

  const { react, pendingId } = useCardReaction({ api, queueReaction });

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
  const rings = storyRings(items, seen);

  return (
    <Screen testID="feed">
      {header ?? (
        <View style={styles.header}>
          {onBack ? (
            <IconButton
              icon="back"
              variant="surface"
              accessibilityLabel={t('common.back')}
              onPress={onBack}
            />
          ) : null}
          <Text variant="display.md" accessibilityRole="header" style={styles.title}>
            {t('feed.title')}
          </Text>
        </View>
      )}
      {onOpen ? <StoryRings rings={rings} onOpen={onOpen} /> : null}
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
        <View style={[styles.block, styles.empty]} testID="feed-empty">
          <Text variant="display.md" style={styles.centerText}>
            {t('feed.empty.title')}
          </Text>
          <Text tone="muted" style={styles.centerText}>
            {t('feed.empty')}
          </Text>
          <Button label={t('feed.compose')} onPress={onCompose} />
        </View>
      ) : null}
      {items.map((item) => (
        <FeedRow
          key={item.id}
          item={item}
          now={at}
          reactions={item.isMine ? [] : shownReactionIds}
          busy={pendingId === item.id}
          onImageError={refreshUrls}
          onOpen={onOpen ? () => onOpen(item.id) : undefined}
          onReact={(kind) => react(item, kind)}
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
      {bottomInset > 0 ? <View style={{ height: bottomInset }} /> : null}
    </Screen>
  );
}

function CardFrame({ children, testID }: { children: ReactNode; testID: string }) {
  const theme = useTheme();
  return (
    <View
      testID={testID}
      style={[styles.card, { backgroundColor: theme.color.bg.paper, boxShadow: theme.shadow(2) }]}
    >
      {children}
    </View>
  );
}

/** Sender, time and caption over the picture, white on the top and bottom scrims (Stories chrome). */
function MediaChrome({
  avatarId,
  name,
  sent,
  caption,
}: {
  avatarId: string;
  name: string;
  sent: string;
  caption: string | null;
}) {
  return (
    <>
      <View style={[styles.chromeTop, { backgroundImage: linearGradient('scrimTop') }]}>
        <Avatar id={avatarId} name={name} size={size.avatar.md} ring="seen" />
        <View style={styles.shrink}>
          <Text variant="label.lg" tone="onMedia" numberOfLines={1}>
            {name}
          </Text>
          <Text variant="label.sm" tone="onMediaMuted">
            {sent}
          </Text>
        </View>
      </View>
      {caption ? (
        <View style={[styles.chromeBottom, { backgroundImage: linearGradient('scrimBottom') }]}>
          <Text variant="hand.md" tone="onMedia">
            {caption}
          </Text>
        </View>
      ) : null}
    </>
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
      <Pressable
        testID={`feed-open-${item.id}`}
        disabled={!onOpen}
        onPress={onOpen}
        accessible
        accessibilityRole={onOpen ? 'button' : 'image'}
        accessibilityLabel={label}
        accessibilityHint={onOpen ? t('feed.card.openHint') : undefined}
        style={[styles.media, { backgroundColor: theme.color.bg.paperAlt }]}
      >
        {live ? (
          <>
            {/* The same picture, blurred, fills the frame behind the card (no letterbox bars). */}
            <Image
              source={{ uri: live.renderUrl }}
              style={StyleSheet.absoluteFill}
              resizeMode="cover"
              blurRadius={24}
              accessibilityIgnoresInvertColors
            />
            <Image
              testID={`feed-image-${item.id}`}
              source={{ uri: live.renderUrl }}
              style={styles.image}
              resizeMode="contain"
              accessibilityIgnoresInvertColors
              onError={onImageError}
            />
          </>
        ) : (
          <View testID={`feed-placeholder-${item.id}`} style={styles.image} />
        )}
        <MediaChrome
          avatarId={item.sender.id}
          name={name}
          sent={
            item.replyTo
              ? t('card.reply.label', { when: sentLabel(item.sentAt, now) })
              : sentLabel(item.sentAt, now)
          }
          caption={item.caption}
        />
        {item.aiGenerated ? <AiBadge testID={`feed-ai-${item.id}`} style={styles.aiBadge} /> : null}
      </Pressable>
      {reactions.length > 0 ? (
        <View style={styles.reactions}>
          <ReactionBar
            reactions={reactions}
            chosen={item.myReaction}
            busy={busy}
            onReact={onReact}
          />
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
      <View
        style={[styles.media, styles.offlineImage, { backgroundColor: theme.color.bg.paperAlt }]}
        accessible
        accessibilityLabel={item.altText ?? t('feed.card.a11y', { name })}
      >
        <Text tone="muted" style={styles.centerText}>
          {t('feed.offline.picture')}
        </Text>
        {/* The offline copy keeps no sender id; the avatar colour follows the name. */}
        <MediaChrome
          avatarId={name}
          name={name}
          sent={sentLabel(item.sentAt, now)}
          caption={item.caption}
        />
      </View>
    </CardFrame>
  );
}

const styles = StyleSheet.create({
  header: { flexDirection: 'row', alignItems: 'center', gap: space[3] },
  title: { flex: 1 },
  block: { gap: space[3] },
  empty: { alignItems: 'center', paddingVertical: space[10] },
  centerText: { textAlign: 'center' },
  card: { borderRadius: radius.xl, overflow: 'hidden' },
  media: { width: '100%', aspectRatio: 1 / CARD_ASPECT, overflow: 'hidden' },
  image: { width: '100%', height: '100%' },
  offlineImage: { alignItems: 'center', justifyContent: 'center', padding: space[8] },
  chromeTop: {
    position: 'absolute',
    top: 0,
    start: 0,
    end: 0,
    flexDirection: 'row',
    alignItems: 'center',
    gap: space[2],
    padding: space[3],
    paddingBottom: space[6],
  },
  chromeBottom: {
    position: 'absolute',
    bottom: 0,
    start: 0,
    end: 0,
    padding: space[4],
    paddingTop: space[8],
  },
  shrink: { flexShrink: 1 },
  reactions: { padding: space[3] },
  aiBadge: { position: 'absolute', top: space[4], end: space[3] },
});
