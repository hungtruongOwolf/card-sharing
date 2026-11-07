import { useMutation, useQueryClient, type InfiniteData } from '@tanstack/react-query';

import { t } from '../../i18n';
import { useToast } from '../../ui';
import { AuthError } from '../auth';
import {
  CardsError,
  isRetryable,
  type CardItem,
  type CardsApi,
  type FeedPage,
} from '../sharing/cardsApi';
import { cardQueryKey, FEED_QUERY_KEY } from './queryKeys';

type FeedData = InfiniteData<FeedPage, string | null>;

const sessionEnded = (error: unknown): boolean =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

function feedWith(data: FeedData | undefined, cardId: string, kind: string | null) {
  if (!data) return data;
  return {
    ...data,
    pages: data.pages.map((page) => ({
      ...page,
      items: page.items.map((item) => (item.id === cardId ? { ...item, myReaction: kind } : item)),
    })),
  };
}

export interface CardReactionOptions {
  api: CardsApi;
  /** Offline outbox (T-78): a reaction that fails for a retryable reason is queued instead of undone. */
  queueReaction?: (cardId: string, reaction: string | null) => Promise<boolean>;
}

/**
 * Private reactions (US-17: only the sender sees them), shared by the feed and the story viewer. The choice shows at
 * once in both the feed and the open card; on failure it is queued when offline, otherwise undone with a toast.
 * Tapping the chosen reaction again removes it.
 */
export function useCardReaction({ api, queueReaction }: CardReactionOptions) {
  const toast = useToast();
  const queryClient = useQueryClient();
  const apply = (cardId: string, kind: string | null) => {
    queryClient.setQueryData<FeedData>(FEED_QUERY_KEY, (data) => feedWith(data, cardId, kind));
    queryClient.setQueryData<CardItem>(cardQueryKey(cardId), (card) =>
      card ? { ...card, myReaction: kind } : card,
    );
  };
  const mutation = useMutation({
    mutationFn: ({ card, kind }: { card: CardItem; kind: string | null }) =>
      kind ? api.setReaction(card.id, kind) : api.clearReaction(card.id),
    onMutate: async ({ card, kind }) => {
      await queryClient.cancelQueries({ queryKey: FEED_QUERY_KEY });
      apply(card.id, kind);
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
      apply(card.id, card.myReaction);
      if (sessionEnded(error)) return;
      const gone = error instanceof CardsError && error.kind === 'not_found';
      toast.show({
        message: t(gone ? 'feed.error.gone' : 'feed.error.react'),
        variant: 'error',
      });
      if (gone) void queryClient.invalidateQueries({ queryKey: FEED_QUERY_KEY });
    },
  });
  return {
    /** Sets `kind`, or removes it when it is already my reaction. */
    react: (card: CardItem, kind: string) =>
      mutation.mutate({ card, kind: kind === card.myReaction ? null : kind }),
    /** The card whose reaction is on its way, if any. */
    pendingId: mutation.isPending ? (mutation.variables?.card.id ?? null) : null,
  };
}
