import { useLocalSearchParams, useRouter } from 'expo-router';
import { useCallback } from 'react';

import {
  appBoardTextures,
  BoardScreen,
  devicePinDisclosure,
  useBoardMembersApi,
  useBoardRepository,
  usePublicLinks,
} from '../../features/boards';
import { useFriendsApi } from '../../features/friends';
import { useRemoteConfig } from '../../features/remote-config';
import { useReportSheet } from '../../features/reports';
import { useCardsApi } from '../../features/sharing';

/** One board on the canvas (T-103/T-104, US-18). */
export default function BoardRoute() {
  const router = useRouter();
  const remoteConfig = useRemoteConfig();
  const cardsApi = useCardsApi();
  const repository = useBoardRepository();
  const members = useBoardMembersApi();
  const publicLinks = usePublicLinks();
  const friendsApi = useFriendsApi();
  const friends = useCallback(() => friendsApi.listFriends(), [friendsApi]);
  const { id } = useLocalSearchParams<{ id: string }>();
  const report = useReportSheet();
  // The newest feed page: cards I received and my own sent cards the server lets me see.
  const pinnableCards = useCallback(async () => (await cardsApi.feed()).items, [cardsApi]);
  return (
    <>
      <BoardScreen
        repository={repository}
        boardId={typeof id === 'string' ? id : ''}
        textures={appBoardTextures()}
        pinnableCards={pinnableCards}
        members={members}
        friends={friends}
        disclosure={devicePinDisclosure()}
        publicLinks={publicLinks}
        remoteConfig={remoteConfig?.registry}
        onReport={(boardId) => report.open({ kind: 'board', id: boardId })}
        onClose={() => (router.canGoBack() ? router.back() : router.replace('/boards'))}
      />
      {report.sheet}
    </>
  );
}
