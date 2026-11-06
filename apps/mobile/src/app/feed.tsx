import { useRouter } from 'expo-router';

import { deviceFeedCache, FeedScreen } from '../features/feed';
import { useOutbox } from '../features/outbox';
import { useRemoteConfig } from '../features/remote-config';
import { useCardsApi } from '../features/sharing';

/** Cards friends sent me, newest first, with an offline copy and private reactions (T-80, T-84); offline reactions are queued (T-78); a tap opens the card full screen (T-81). */
export default function FeedRoute() {
  const router = useRouter();
  const api = useCardsApi();
  const remoteConfig = useRemoteConfig();
  const outbox = useOutbox()?.outbox;
  return (
    <FeedScreen
      api={api}
      cache={deviceFeedCache()}
      reactions={remoteConfig?.reactions}
      queueReaction={outbox ? (cardId, kind) => outbox.enqueueReaction(cardId, kind) : undefined}
      onOpen={(cardId) => router.push({ pathname: '/card/[id]', params: { id: cardId } })}
      onCompose={() => router.push('/capture')}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
    />
  );
}
