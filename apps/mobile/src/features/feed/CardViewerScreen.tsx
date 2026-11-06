import { useImage } from '@shopify/react-native-skia';
import { useQuery, useQueryClient, type InfiniteData } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { Image, StyleSheet, useWindowDimensions, View } from 'react-native';
import { useSharedValue, type SharedValue } from 'react-native-reanimated';

import {
  killedIds,
  registry as shippedRegistry,
  resolveParams,
  type EffectEntry,
  type Registry,
  type RemoteRegistryConfig,
} from '@card-sharing/effects-registry';
import { radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text, useTheme, useToast } from '../../ui';
import { AuthError } from '../auth';
import { LiveFinishPreview } from '../editor/LiveFinishPreview';
import { planFinishModes } from '../editor/liveFinish';
import { Squish } from '../feedback';
import {
  CardsError,
  type CardItem,
  type CardMedia,
  type CardsApi,
  type FeedPage,
} from '../sharing/cardsApi';
import { FEED_QUERY_KEY } from './FeedScreen';

export const cardQueryKey = (id: string) => ['card', id] as const;

export interface CardViewerScreenProps {
  api: CardsApi;
  /** From the route; anything that is not a card id reads as a removed card. */
  cardId: string;
  registry?: Registry;
  /** `/v1/config` registry part: a killed finish is drawn static. */
  remoteConfig?: RemoteRegistryConfig;
  /** Device clock seam for tests (signed-URL expiries are on the device clock, see `createCardsApi`). */
  now?: () => number;
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

const sessionEnded = (error: unknown): boolean =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

const removed = (error: unknown): boolean =>
  error instanceof CardsError && error.kind === 'not_found';

function fromFeed(
  data: InfiniteData<FeedPage, unknown> | undefined,
  cardId: string,
): CardItem | undefined {
  for (const page of data?.pages ?? []) {
    const item = page.items.find((card) => card.id === cardId);
    if (item) return item;
  }
  return undefined;
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

/**
 * A received (or my own) card full screen (T-81, US-15): the baked render with its finish drawn live where the
 * registry allows it (one card on screen, inside the `cost.maxLive` budget; static under reduced motion), wrapped
 * in `Squish`. The card is read with `GET /v1/cards/{id}` for fresh signed URLs; the feed's copy shows meanwhile.
 * A card that is gone (deleted, unfriended, blocked: the server answers 404 for all) says so and closes.
 */
export function CardViewerScreen({
  api,
  cardId,
  registry = shippedRegistry,
  remoteConfig,
  now = Date.now,
  onClose,
}: CardViewerScreenProps) {
  const toast = useToast();
  const theme = useTheme();
  const queryClient = useQueryClient();
  const screenSize = useWindowDimensions();
  const press = useSharedValue(0);
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  const lastUrlRefresh = useRef<number | null>(null);
  const closed = useRef(false);

  const query = useQuery({
    queryKey: cardQueryKey(cardId),
    queryFn: () => api.getCard(cardId),
    placeholderData: () =>
      fromFeed(queryClient.getQueryData<InfiniteData<FeedPage>>(FEED_QUERY_KEY), cardId),
    refetchInterval: CARD_RECHECK_MS,
    retry: (count, error) => !removed(error) && !sessionEnded(error) && count < 1,
  });
  const card = query.data;
  const media = useShownMedia(
    card?.media ?? null,
    now,
    failedUrl !== null && failedUrl === card?.media?.renderUrl,
  );

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
  const width = Math.min(screenSize.width - space[4] * 2, (screenSize.height * 0.7) / CARD_ASPECT);
  const size = { width, height: width * CARD_ASPECT };
  const name = card?.sender.displayName ?? t('feed.someone');
  const label = card?.altText ?? t('feed.card.a11y', { name });

  return (
    <Screen testID="card-viewer">
      <Button
        variant="ghost"
        label={t('card.close')}
        onPress={onClose}
        testID="card-viewer-close"
      />
      {!card && query.isPending ? (
        <Text tone="muted" testID="card-viewer-loading">
          {t('card.loading')}
        </Text>
      ) : null}
      {!card && query.isError && !removed(query.error) ? (
        <View style={styles.block} testID="card-viewer-error">
          <Text accessibilityRole="alert">{t('card.error.load')}</Text>
          <Button label={t('feed.retry')} onPress={() => void query.refetch()} />
        </View>
      ) : null}
      {card ? (
        <>
          <Squish style={styles.center} press={press} gestureTestId="card-viewer-squish">
            <View
              testID="card-viewer-card"
              style={[styles.card, size, { backgroundColor: theme.color.bg.paperAlt }]}
              accessible
              accessibilityRole="image"
              accessibilityLabel={label}
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
                      size={size}
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
          </Squish>
          <Text variant="label.md">{name}</Text>
          {card.caption ? <Text variant="hand.md">{card.caption}</Text> : null}
        </>
      ) : null}
    </Screen>
  );
}

/**
 * The live finish over the render (T-65 renderer, gyro + ambient shimmer on the UI thread). The render already has
 * the finish baked at the resting tilt; the live layer draws the card again with a moving shine, so with the sensor
 * at rest it looks the same. Shown only once Skia has the picture: until then (or if it fails) the static render
 * stays.
 */
function LiveShine({
  url,
  size,
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
        width={size.width}
        height={size.height}
        intensity={intensity}
        press={press}
        testID="card-viewer-live-finish"
      />
    </View>
  );
}

const styles = StyleSheet.create({
  block: { gap: space[3] },
  center: { alignSelf: 'center' },
  card: {
    borderRadius: radius.md,
    overflow: 'hidden',
    alignItems: 'center',
    justifyContent: 'center',
  },
  fill: { width: '100%', height: '100%' },
  overlay: StyleSheet.absoluteFill,
});
