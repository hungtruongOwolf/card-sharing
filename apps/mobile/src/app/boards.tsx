import { useRouter } from 'expo-router';

import { appBoardRepository, BoardsScreen } from '../features/boards';

/** My boards (US-18). */
export default function BoardsRoute() {
  const router = useRouter();
  return (
    <BoardsScreen
      repository={appBoardRepository()}
      onOpen={(id) => router.push({ pathname: '/board/[id]', params: { id } })}
      onOpenLab={() => router.push('/board-lab')}
    />
  );
}
