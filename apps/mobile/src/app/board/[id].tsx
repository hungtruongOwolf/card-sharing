import { useLocalSearchParams, useRouter } from 'expo-router';

import { appBoardRepository, appBoardTextures, BoardScreen } from '../../features/boards';
import { useRemoteConfig } from '../../features/remote-config';

/** One board on the canvas (T-103, US-18). */
export default function BoardRoute() {
  const router = useRouter();
  const remoteConfig = useRemoteConfig();
  const { id } = useLocalSearchParams<{ id: string }>();
  return (
    <BoardScreen
      repository={appBoardRepository()}
      boardId={typeof id === 'string' ? id : ''}
      textures={appBoardTextures()}
      remoteConfig={remoteConfig?.registry}
      onClose={() => (router.canGoBack() ? router.back() : router.replace('/boards'))}
    />
  );
}
