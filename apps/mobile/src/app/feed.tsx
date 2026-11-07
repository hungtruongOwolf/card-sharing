import { useRouter } from 'expo-router';

import { deviceFeedCache, deviceSeenStore, FeedScreen, useSeenMarks } from '../features/feed';
import { useOutbox } from '../features/outbox';
import { useRemoteConfig } from '../features/remote-config';
import { useCardsApi } from '../features/sharing';

/** Cards friends sent me with story rings, an offline copy and private reactions (T-80, T-84, T-78); a tap opens the story viewer (T-81). */
export default function FeedRoute() {
  const router = useRouter();
  const api = useCardsApi();
  const remoteConfig = useRemoteConfig();
  const outbox = useOutbox()?.outbox;
  const seen = useSeenMarks(deviceSeenStore());
  return (
    <FeedScreen
      api={api}
      cache={deviceFeedCache()}
      reactions={remoteConfig?.reactions}
      queueReaction={outbox ? (cardId, kind) => outbox.enqueueReaction(cardId, kind) : undefined}
      seen={seen}
      onOpen={(cardId) => router.push({ pathname: '/card/[id]', params: { id: cardId } })}
      onCompose={() => router.push('/capture')}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
    />
  );
}
