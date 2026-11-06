import { useRouter } from 'expo-router';

import { BoardsScreen, useBoardRepository } from '../features/boards';

/** My boards (US-18). */
export default function BoardsRoute() {
  const router = useRouter();
  const repository = useBoardRepository();
  return (
    <BoardsScreen
      repository={repository}
      onOpen={(id) => router.push({ pathname: '/board/[id]', params: { id } })}
      onOpenLab={() => router.push('/board-lab')}
    />
  );
}
