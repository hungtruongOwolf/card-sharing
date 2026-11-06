import { Redirect, useLocalSearchParams, useRouter } from 'expo-router';
import { useCallback, useEffect, useMemo, useState } from 'react';

import { registry } from '@card-sharing/effects-registry';

import { prepareForSend, type LocalCard } from '../features/cards';
import { deviceDraftStore } from '../features/drafts';
import { createSyntheticMaskSource, noMaskSource } from '../features/editor';
import { useFriendsApi } from '../features/friends';
import { useOutbox } from '../features/outbox';
import { useRemoteConfig } from '../features/remote-config';
import { SendScreen, useCardsApi } from '../features/sharing';
import { t } from '../i18n';
import { Screen, Text } from '../ui';

/**
 * Send a draft (T-77): `/send?draft=<id>` from the editor's Next button or the drafts tray. The card is read
 * from the drafts tray on this device; the route param is only an id, never card content.
 */
export default function SendRoute() {
  const router = useRouter();
  const params = useLocalSearchParams();
  const draftId = typeof params.draft === 'string' ? params.draft : null;
  const [card, setCard] = useState<LocalCard | null | 'loading'>(draftId ? 'loading' : null);
  const friendsApi = useFriendsApi();
  const cardsApi = useCardsApi();
  const remoteConfig = useRemoteConfig();
  const outbox = useOutbox()?.outbox;
  // The same mask source as the editor, so the sent card matches what the user styled.
  const maskSource = useMemo(() => (__DEV__ ? createSyntheticMaskSource() : noMaskSource), []);
  const prepare = useCallback(
    (local: LocalCard, keepOriginal: boolean) =>
      prepareForSend(local, keepOriginal, { registry, maskSource }),
    [maskSource],
  );

  useEffect(() => {
    if (!draftId) return;
    let cancelled = false;
    deviceDraftStore()
      .get(draftId)
      .then(
        (found) => !cancelled && setCard(found),
        () => !cancelled && setCard(null),
      );
    return () => {
      cancelled = true;
    };
  }, [draftId]);

  if (card === 'loading') {
    return (
      <Screen centered>
        <Text tone="muted">{t('editor.loading')}</Text>
      </Screen>
    );
  }
  if (!card) return <Redirect href="/drafts" />;
  return (
    <SendScreen
      card={card}
      cardsApi={cardsApi}
      friendsApi={friendsApi}
      prepare={prepare}
      removeDraft={(id) => deviceDraftStore().remove(id)}
      registry={registry}
      recipientsMax={remoteConfig?.cardRecipientsMax}
      queue={outbox ? (job) => outbox.enqueueSend(job) : undefined}
      onEdit={() => router.replace({ pathname: '/editor', params: { draft: card.id } })}
      onOpenFriends={() => router.push('/friends')}
      onDone={() => router.replace('/')}
    />
  );
}
