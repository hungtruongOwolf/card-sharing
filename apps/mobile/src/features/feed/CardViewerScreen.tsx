import { Skia, useImage, type SkImage } from '@shopify/react-native-skia';
import { useQuery, useQueryClient, type InfiniteData } from '@tanstack/react-query';
import { StatusBar } from 'expo-status-bar';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import {
  AccessibilityInfo,
  I18nManager,
  Image,
  StyleSheet,
  useWindowDimensions,
  View,
  type AccessibilityActionEvent,
} from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, {
  useAnimatedStyle,
  useSharedValue,
  type SharedValue,
} from 'react-native-reanimated';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { scheduleOnRN } from 'react-native-worklets';

import {
  killedIds,
  registry as shippedRegistry,
  resolveParams,
  type EffectEntry,
  type Registry,
  type RemoteRegistryConfig,
} from '@card-sharing/effects-registry';
import {
  gesture as gestureTokens,
  linearGradient,
  motion,
  palette,
  radius,
  size,
  space,
} from '@card-sharing/tokens';

import { t } from '../../i18n';
import { animateTo, Avatar, Button, IconButton, Text, useTheme, useToast } from '../../ui';
import { AuthError } from '../auth';
import { LiveFinishPreview } from '../editor/LiveFinishPreview';
import { canShareImages, exportRenderPng, shareImage } from '../export';
import { planFinishModes } from '../editor/liveFinish';
import {
  CardsError,
  type CardItem,
  type CardMedia,
  type CardsApi,
  type FeedPage,
} from '../sharing/cardsApi';
import { sentLabel } from './FeedScreen';
import { cardQueryKey, FEED_QUERY_KEY } from './queryKeys';
import { ReactionBar } from './ReactionBar';
import { shownReactions } from './reactions';
import { storySequence } from './stories';
import { StoryProgress } from './StoryProgress';
import { useCardReaction } from './useCardReaction';
import { useStoryTimer } from './useStoryTimer';

export { cardQueryKey };

export interface CardViewerScreenProps {
  api: CardsApi;
  /** From the route; anything that is not a card id reads as a removed card. */
  cardId: string;
  registry?: Registry;
  /** `/v1/config` registry part: a killed finish is drawn static. */
  remoteConfig?: RemoteRegistryConfig;
  /** Reaction ids from `/v1/config`; none are offered without it (and never on my own cards). */
  reactions?: readonly string[];
  /** Offline outbox (T-78) for reactions. */
  queueReaction?: (cardId: string, reaction: string | null) => Promise<boolean>;
  /** Called once per card shown (story rings mark it seen). */
  onSeen?: (card: CardItem) => void;
  /** Card ids to play; defaults to `storySequence` over the loaded feed. */
  sequence?: readonly string[];
  /** How long one card shows (tests shorten it). */
  segmentMs?: number;
  /** Holds the story timer, e.g. while the report sheet is open over the viewer. */
  paused?: boolean;
  /** Device clock seam for tests (signed-URL expiries are on the device clock, see `createCardsApi`). */
  now?: () => number;
  /** Export seams (T-127): decode the render, share the PNG. */
  loadRender?: (url: string) => Promise<SkImage | null>;
  share?: (png: Uint8Array) => Promise<boolean>;
  /** Report a friend's card (US-26); absent hides the action. Never offered on my own cards (the server says 404). */
  onReport?: (cardId: string) => void;
  onClose: () => void;
}

/** Refetch this long before the shown signed URL expires (same margin as the feed). */
const EXPIRY_SKEW_MS = 30_000;
/** At most one URL refresh per this interval after a failed picture. */
const URL_REFRESH_MIN_INTERVAL_MS = 60_000;
/**
 * While open, the card is re-read this often, so a card deleted, unfriended or blocked while on screen closes
 * within a minute (prd-mvp US-14: removed everywhere within 60 s). 300/min per user is the read budget.
 */
export const CARD_RECHECK_MS = 60_000;
const CARD_ASPECT = 4 / 3;
/** The viewer is always night, like a camera roll: photos are the only colour on screen. */
const STAGE = palette.night[950];

const sessionEnded = (error: unknown): boolean =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

const removed = (error: unknown): boolean =>
  error instanceof CardsError && error.kind === 'not_found';

function feedItems(data: InfiniteData<FeedPage, unknown> | undefined): CardItem[] {
  return data?.pages.flatMap((page) => page.items) ?? [];
}

/**
 * The signed media on screen. A re-read signs new URLs, and the picture would download again on every one, so the
 * shown URL is kept until it is about to expire (or failed to load); then the newest read's media replaces it.
 * Past its expiry with nothing newer, nothing is shown: a picture never outlives its signed-URL TTL (SR-8).
 */
function useShownMedia(latest: CardMedia | null, now: () => number, failed: boolean) {
  const [shown, setShown] = useState<CardMedia | null>(latest);
  const [, setExpiredTick] = useState(0);
  const stale = (media: CardMedia | null) =>
    media === null || media.expiresAt - EXPIRY_SKEW_MS <= now();
  const next =
    latest && (failed || stale(shown)) && latest.renderUrl !== shown?.renderUrl ? latest : shown;
  if (next !== shown) setShown(next);

  const expiresAt = next?.expiresAt ?? null;
  useEffect(() => {
    if (expiresAt === null) return;
    const timer = setTimeout(() => setExpiredTick((n) => n + 1), Math.max(0, expiresAt - now()));
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [expiresAt]);

  return next && next.expiresAt > now() ? next : null;
}

function finishEntry(
  card: CardItem | undefined,
  registry: Registry,
  remoteConfig: RemoteRegistryConfig | undefined,
): { entry: EffectEntry; intensity: number } | null {
  if (!card?.finish) return null;
  const entry = registry.get(card.finish.id);
  if (!entry || entry.kind !== 'filter' || killedIds(remoteConfig).has(entry.id)) return null;
  const params = resolveParams(entry, card.finish.params);
  const intensity = params.ok ? Number(params.params.intensity ?? 1) : 1;
  return { entry, intensity: Number.isFinite(intensity) ? intensity : 1 };
}

/** True while VoiceOver / TalkBack is on: stories then never move on by themselves (WCAG 2.2.2). */
function useScreenReader(): boolean {
  const [enabled, setEnabled] = useState(false);
  useEffect(() => {
    let active = true;
    // Wrapped: some hosts (tests, older web shims) return nothing instead of a promise.
    Promise.resolve(AccessibilityInfo.isScreenReaderEnabled()).then(
      (on) => active && setEnabled(on === true),
      () => undefined,
    );
    const sub = AccessibilityInfo.addEventListener('screenReaderChanged', setEnabled);
    return () => {
      active = false;
      sub.remove();
    };
  }, []);
  return enabled;
}

/**
 * Friends' cards as Stories (T-81, US-15; sprint 9). Full screen on night: progress segments at the top, the sender
 * with an avatar and the time, the card over a blurred copy of itself, reactions at the bottom. Tap the end side for
 * the next card and the start side for the previous one (mirrored in RTL), hold to pause (the hold also squishes a
 * live holo finish), swipe down to close. Each card plays `motion.story.segmentMs`; the order is each friend's cards
 * oldest first, friends in feed order (`storySequence`). With a screen reader on nothing moves by itself: the card
 * offers next / previous actions instead.
 *
 * Each card is read with `GET /v1/cards/{id}` for fresh signed URLs; the feed's copy shows meanwhile. A card that is
 * gone (deleted, unfriended, blocked: the server answers 404 for all) says so and closes the viewer.
 */
export function CardViewerScreen(props: CardViewerScreenProps) {
  const { cardId, paused = false, segmentMs = motion.story.segmentMs, onClose } = props;
  const theme = useTheme();
  const queryClient = useQueryClient();
  const insets = useSafeAreaInsets();
  const { width, height } = useWindowDimensions();
  const screenReader = useScreenReader();
  const reduced = theme.reducedMotion;

  const [sequence] = useState(
    () =>
      props.sequence ??
      storySequence(
        feedItems(queryClient.getQueryData<InfiniteData<FeedPage>>(FEED_QUERY_KEY)),
        cardId,
      ),
  );
  const [index, setIndex] = useState(() => Math.max(0, sequence.indexOf(cardId)));
  const currentId = sequence[index] ?? cardId;
  const [ready, setReady] = useState(false);
  const [holding, setHolding] = useState(false);
  const press = useSharedValue(0);
  const dragY = useSharedValue(0);

  const go = (step: 1 | -1) => {
    const next = index + step;
    if (next >= sequence.length) {
      onClose();
      return;
    }
    if (next < 0) return;
    setReady(false);
    setIndex(next);
  };

  const progress = useStoryTimer({
    resetKey: currentId,
    running: ready && !holding && !paused && !screenReader,
    durationMs: segmentMs,
    onDone: () => go(1),
  });

  const onTap = (x: number) => {
    const startSide = I18nManager.isRTL
      ? x > width * (1 - gestureTokens.backZone)
      : x < width * gestureTokens.backZone;
    go(startSide ? -1 : 1);
  };
  const hold = (on: boolean) => {
    setHolding(on);
    press.set(animateTo(on ? 1 : 0, 'squish', reduced));
  };

  const tap = Gesture.Tap()
    .withTestId('story-tap')
    .maxDuration(motion.story.holdMs)
    .runOnJS(true)
    .onEnd((event, success) => {
      if (success) onTap(event.x);
    });
  const longPress = Gesture.LongPress()
    .withTestId('story-hold')
    .minDuration(motion.story.holdMs)
    .runOnJS(true)
    .onStart(() => hold(true))
    .onFinalize(() => hold(false));
  const dismiss = Gesture.Pan()
    .withTestId('story-dismiss')
    .activeOffsetY(12)
    .failOffsetX([-24, 24])
    .onUpdate((event) => {
      dragY.set(Math.max(0, event.translationY));
    })
    .onEnd((event) => {
      if (
        event.translationY > gestureTokens.dismissDistance ||
        event.velocityY > gestureTokens.dismissVelocity
      ) {
        scheduleOnRN(onClose);
      } else {
        dragY.set(animateTo(0, 'swipe', reduced));
      }
    });
  const gesture = Gesture.Race(dismiss, Gesture.Exclusive(longPress, tap));

  const dragStyle = useAnimatedStyle(() => {
    const y = dragY.get();
    return reduced
      ? { opacity: 1 - Math.min(0.5, y / 600) }
      : {
          transform: [{ translateY: y }, { scale: 1 - Math.min(0.15, y / 1600) }],
          borderRadius: Math.min(radius.xl, y / 4),
        };
  });

  // The media sits inside the gesture view; buttons are drawn above it, outside, so taps on them never page.
  const stage = (children: ReactNode) => (
    <GestureDetector gesture={gesture}>
      <View style={StyleSheet.absoluteFill} collapsable={false}>
        {children}
      </View>
    </GestureDetector>
  );

  const topInset = Math.max(insets.top, space[2]);
  return (
    <View style={[styles.root, { backgroundColor: STAGE }]} testID="card-viewer">
      <StatusBar style="light" />
      <Animated.View style={[styles.fill, styles.clip, dragStyle]}>
        <StoryCard
          key={currentId}
          {...props}
          cardId={currentId}
          screen={{ width, height }}
          insets={{
            top: topInset + size.progress + space[2],
            bottom: Math.max(insets.bottom, space[3]),
          }}
          press={press}
          stage={stage}
          onReady={() => setReady(true)}
          a11yActions={{
            hasNext: index < sequence.length - 1,
            hasPrevious: index > 0,
            onNext: () => go(1),
            onPrevious: () => go(-1),
          }}
        />
        <View style={[styles.top, { paddingTop: topInset }]} pointerEvents="box-none">
          {sequence.length > 1 ? (
            <StoryProgress count={sequence.length} index={index} progress={progress} />
          ) : null}
        </View>
        <View
          style={[styles.close, { top: topInset + size.progress + space[2] }]}
          pointerEvents="box-none"
        >
          <IconButton
            icon="close"
            accessibilityLabel={t('card.close')}
            onPress={onClose}
            testID="card-viewer-close"
          />
        </View>
      </Animated.View>
    </View>
  );
}

interface StoryCardProps extends CardViewerScreenProps {
  screen: { width: number; height: number };
  insets: { top: number; bottom: number };
  press: SharedValue<number>;
  stage: (children: ReactNode) => ReactNode;
  onReady: () => void;
  a11yActions: {
    hasNext: boolean;
    hasPrevious: boolean;
    onNext: () => void;
    onPrevious: () => void;
  };
}

/** One card of the story: its read, media, finish, chrome and actions. Keyed by card id, so state never leaks. */
function StoryCard({
  api,
  cardId,
  registry = shippedRegistry,
  remoteConfig,
  reactions,
  queueReaction,
  onSeen,
  now = Date.now,
  loadRender = loadRenderFromUrl,
  share = shareImage,
  onReport,
  onClose,
  screen,
  insets,
  press,
  stage,
  onReady,
  a11yActions,
}: StoryCardProps) {
  const toast = useToast();
  const theme = useTheme();
  const queryClient = useQueryClient();
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  const lastUrlRefresh = useRef<number | null>(null);
  const closed = useRef(false);
  const seenOnce = useRef(false);

  const query = useQuery({
    queryKey: cardQueryKey(cardId),
    queryFn: () => api.getCard(cardId),
    placeholderData: () =>
      feedItems(queryClient.getQueryData<InfiniteData<FeedPage>>(FEED_QUERY_KEY)).find(
        (item) => item.id === cardId,
      ),
    refetchInterval: CARD_RECHECK_MS,
    retry: (count, error) => !removed(error) && !sessionEnded(error) && count < 1,
  });
  const card = query.data;
  const media = useShownMedia(
    card?.media ?? null,
    now,
    failedUrl !== null && failedUrl === card?.media?.renderUrl,
  );
  const { react, pendingId } = useCardReaction({ api, queueReaction });

  const { error } = query;
  useEffect(() => {
    if (!removed(error) || closed.current) return;
    closed.current = true;
    queryClient.removeQueries({ queryKey: cardQueryKey(cardId) });
    void queryClient.invalidateQueries({ queryKey: FEED_QUERY_KEY });
    toast.show({ message: t('card.removed') });
    onClose();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [error]);

  // The segment starts once there is something to look at.
  const hasCard = card !== undefined;
  useEffect(() => {
    if (!hasCard) return;
    onReady();
    if (card && !seenOnce.current) {
      seenOnce.current = true;
      onSeen?.(card);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasCard]);

  /** Re-reads the card for new signed URLs, at most once a minute (a picture that keeps failing must not loop). */
  const refreshUrls = () => {
    const at = now();
    const last = lastUrlRefresh.current;
    if (query.isFetching || (last !== null && at - last < URL_REFRESH_MIN_INTERVAL_MS)) return;
    lastUrlRefresh.current = at;
    void query.refetch();
  };

  const expiresAt = media?.expiresAt ?? null;
  useEffect(() => {
    if (expiresAt === null) return;
    const timer = setTimeout(refreshUrls, Math.max(0, expiresAt - EXPIRY_SKEW_MS - now()));
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [expiresAt]);

  const finish = finishEntry(card, registry, remoteConfig);
  const [mode] = planFinishModes([finish?.entry ?? null], {
    reducedMotion: theme.reducedMotion,
  });
  const chromeHeight = insets.top + size.avatar.md + space[6] + insets.bottom + 120;
  const width = Math.min(
    screen.width - space[4] * 2,
    Math.max(160, (screen.height - chromeHeight) / CARD_ASPECT),
  );
  const cardSize = { width, height: width * CARD_ASPECT };
  const name = card?.sender.displayName ?? t('feed.someone');
  const label = card?.altText ?? t('feed.card.a11y', { name });

  const actions = [
    ...(a11yActions.hasNext ? [{ name: 'next', label: t('card.viewer.next') }] : []),
    ...(a11yActions.hasPrevious ? [{ name: 'previous', label: t('card.viewer.previous') }] : []),
  ];
  const onAction = (event: AccessibilityActionEvent) => {
    if (event.nativeEvent.actionName === 'next') a11yActions.onNext();
    if (event.nativeEvent.actionName === 'previous') a11yActions.onPrevious();
  };

  const offered = card && !card.isMine ? shownReactions(reactions) : [];

  return (
    <>
      {stage(
        <>
          {media ? (
            <Image
              source={{ uri: media.renderUrl }}
              style={StyleSheet.absoluteFill}
              resizeMode="cover"
              blurRadius={40}
              accessibilityIgnoresInvertColors
            />
          ) : null}
          <View style={[StyleSheet.absoluteFill, styles.dim]} />
          <View style={[styles.fill, styles.centered]}>
            {card ? (
              <View
                testID="card-viewer-card"
                style={[styles.card, cardSize, { backgroundColor: theme.color.bg.paperAlt }]}
                accessible
                accessibilityRole="image"
                accessibilityLabel={label}
                accessibilityHint={t('card.viewer.hint')}
                accessibilityActions={actions}
                onAccessibilityAction={onAction}
                accessibilityIgnoresInvertColors
              >
                {media ? (
                  <>
                    <Image
                      testID="card-viewer-image"
                      source={{ uri: media.renderUrl }}
                      style={styles.fill}
                      resizeMode="contain"
                      onError={() => {
                        setFailedUrl(media.renderUrl);
                        refreshUrls();
                      }}
                    />
                    {mode === 'live' && finish ? (
                      <LiveShine
                        url={media.renderUrl}
                        size={cardSize}
                        intensity={finish.intensity}
                        press={press}
                      />
                    ) : null}
                  </>
                ) : (
                  <Text tone="muted" testID="card-viewer-placeholder">
                    {t('feed.offline.picture')}
                  </Text>
                )}
              </View>
            ) : query.isPending ? (
              <Text tone="onMediaMuted" testID="card-viewer-loading">
                {t('card.loading')}
              </Text>
            ) : null}
          </View>
        </>,
      )}
      {!card && query.isError && !removed(query.error) ? (
        <View style={[styles.fill, styles.centered, styles.errorBox]} testID="card-viewer-error">
          <Text tone="onMedia" accessibilityRole="alert" style={styles.centerText}>
            {t('card.error.load')}
          </Text>
          <Button label={t('feed.retry')} onPress={() => void query.refetch()} />
        </View>
      ) : null}
      {card ? (
        <>
          <View
            pointerEvents="box-none"
            style={[
              styles.header,
              { paddingTop: insets.top, backgroundImage: linearGradient('scrimTop') },
            ]}
          >
            <Avatar id={card.sender.id} name={name} size={size.avatar.md} ring="seen" />
            <View style={styles.shrink}>
              <Text variant="label.lg" tone="onMedia" numberOfLines={1}>
                {name}
              </Text>
              <Text variant="label.sm" tone="onMediaMuted">
                {sentLabel(card.sentAt, now())}
              </Text>
            </View>
          </View>
          <View
            pointerEvents="box-none"
            style={[
              styles.footer,
              { paddingBottom: insets.bottom, backgroundImage: linearGradient('scrimBottom') },
            ]}
          >
            {card.caption ? (
              <Text variant="hand.md" tone="onMedia" style={styles.centerText}>
                {card.caption}
              </Text>
            ) : null}
            <View style={styles.actions} pointerEvents="box-none">
              <View style={styles.shrink}>
                <ReactionBar
                  tone="media"
                  reactions={offered}
                  chosen={card.myReaction}
                  busy={pendingId === card.id}
                  onReact={(kind) => react(card, kind)}
                />
              </View>
              {/* Export is for the owner's own cards only (AC-23.2): a friend's card never leaves the device here. */}
              {card.isMine && media && canShareImages() ? (
                <IconButton
                  icon="download"
                  accessibilityLabel={t('card.export')}
                  accessibilityHint={t('card.export.hint')}
                  onPress={async () => {
                    const image = await loadRender(media.renderUrl).catch(() => null);
                    const png = image ? exportRenderPng(image) : null;
                    if (!png || !(await share(png).catch(() => false))) {
                      toast.show({ message: t('card.export.failed') });
                    }
                  }}
                />
              ) : null}
              {!card.isMine && onReport ? (
                <IconButton
                  icon="flag"
                  accessibilityLabel={t('report.open.card', { name })}
                  onPress={() => onReport(card.id)}
                />
              ) : null}
            </View>
          </View>
        </>
      ) : null}
    </>
  );
}

/** Decodes a sent card's render (a signed URL) with Skia for export. */
const loadRenderFromUrl = async (url: string) =>
  Skia.Image.MakeImageFromEncoded(await Skia.Data.fromURI(url));

/**
 * The live finish over the render (T-65 renderer, gyro + ambient shimmer on the UI thread). The render already has
 * the finish baked at the resting tilt; the live layer draws the card again with a moving shine, so with the sensor
 * at rest it looks the same. Shown only once Skia has the picture: until then (or if it fails) the static render
 * stays.
 */
function LiveShine({
  url,
  size: box,
  intensity,
  press,
}: {
  url: string;
  size: { width: number; height: number };
  intensity: number;
  press: SharedValue<number>;
}) {
  const image = useImage(url);
  if (!image) return null;
  return (
    <View style={styles.overlay} pointerEvents="none">
      <LiveFinishPreview
        card={{ image }}
        width={box.width}
        height={box.height}
        intensity={intensity}
        press={press}
        testID="card-viewer-live-finish"
      />
    </View>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1 },
  fill: { flex: 1 },
  clip: { overflow: 'hidden' },
  centered: { alignItems: 'center', justifyContent: 'center' },
  centerText: { textAlign: 'center' },
  dim: { backgroundColor: 'rgba(0, 0, 0, 0.35)' },
  card: {
    borderRadius: radius.xl,
    overflow: 'hidden',
    alignItems: 'center',
    justifyContent: 'center',
  },
  overlay: StyleSheet.absoluteFill,
  top: { position: 'absolute', top: 0, start: 0, end: 0, paddingHorizontal: space[2] },
  close: { position: 'absolute', end: space[2] },
  header: {
    position: 'absolute',
    top: 0,
    start: 0,
    end: 0,
    flexDirection: 'row',
    alignItems: 'center',
    gap: space[2],
    paddingHorizontal: space[3],
    paddingEnd: space[16],
    paddingBottom: space[6],
  },
  footer: {
    position: 'absolute',
    bottom: 0,
    start: 0,
    end: 0,
    gap: space[3],
    paddingHorizontal: space[3],
    paddingTop: space[10],
  },
  actions: { flexDirection: 'row', alignItems: 'center', gap: space[2] },
  shrink: { flex: 1, flexShrink: 1 },
  errorBox: { ...StyleSheet.absoluteFill, gap: space[3], padding: space[6] },
});
