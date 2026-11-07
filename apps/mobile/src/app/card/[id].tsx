import { useLocalSearchParams, useRouter } from 'expo-router';

import { CardViewerScreen, deviceSeenStore } from '../../features/feed';
import { useOutbox } from '../../features/outbox';
import { useRemoteConfig } from '../../features/remote-config';
import { useReportSheet } from '../../features/reports';
import { useCardsApi } from '../../features/sharing';

/** Friends' cards as a story, opened at one card (T-81, US-15); opened from the feed or a story ring. */
export default function CardRoute() {
  const router = useRouter();
  const api = useCardsApi();
  const remoteConfig = useRemoteConfig();
  const outbox = useOutbox()?.outbox;
  const { id } = useLocalSearchParams<{ id: string }>();
  const report = useReportSheet();
  return (
    <>
      <CardViewerScreen
        api={api}
        cardId={typeof id === 'string' ? id : ''}
        remoteConfig={remoteConfig?.registry}
        reactions={remoteConfig?.reactions}
        queueReaction={outbox ? (cardId, kind) => outbox.enqueueReaction(cardId, kind) : undefined}
        onSeen={(card) => {
          if (!card.isMine) void deviceSeenStore().markSeen(card.sender.id, card.sentAt);
        }}
        paused={report.isOpen}
        onReport={(cardId) => report.open({ kind: 'card', id: cardId })}
        onClose={() => (router.canGoBack() ? router.back() : router.replace('/'))}
      />
      {report.sheet}
    </>
  );
}
