import { useRouter } from 'expo-router';

import { deviceFeedCache, FeedScreen } from '../features/feed';
import { useRemoteConfig } from '../features/remote-config';
import { useCardsApi } from '../features/sharing';

/** Cards friends sent me, newest first, with an offline copy and private reactions (T-80, T-84). */
export default function FeedRoute() {
  const router = useRouter();
  const api = useCardsApi();
  const remoteConfig = useRemoteConfig();
  return (
    <FeedScreen
      api={api}
      cache={deviceFeedCache()}
      reactions={remoteConfig?.reactions}
      onCompose={() => router.push('/capture')}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
    />
  );
}
