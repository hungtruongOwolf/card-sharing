import { useLocalSearchParams, useRouter } from 'expo-router';
import { useCallback } from 'react';

import { appBoardRepository, appBoardTextures, BoardScreen } from '../../features/boards';
import { useRemoteConfig } from '../../features/remote-config';
import { useCardsApi } from '../../features/sharing';

/** One board on the canvas (T-103/T-104, US-18). */
export default function BoardRoute() {
  const router = useRouter();
  const remoteConfig = useRemoteConfig();
  const cardsApi = useCardsApi();
  const { id } = useLocalSearchParams<{ id: string }>();
  // The newest feed page: cards I received and my own sent cards the server lets me see.
  const pinnableCards = useCallback(async () => (await cardsApi.feed()).items, [cardsApi]);
  return (
    <BoardScreen
      repository={appBoardRepository()}
      boardId={typeof id === 'string' ? id : ''}
      textures={appBoardTextures()}
      pinnableCards={pinnableCards}
      remoteConfig={remoteConfig?.registry}
      onClose={() => (router.canGoBack() ? router.back() : router.replace('/boards'))}
    />
  );
}
