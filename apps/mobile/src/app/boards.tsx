import { useRouter } from 'expo-router';

import { BoardsScreen, useBoardMembersApi, useBoardRepository } from '../features/boards';

/** My boards (US-18). */
export default function BoardsRoute() {
  const router = useRouter();
  const repository = useBoardRepository();
  const invitations = useBoardMembersApi();
  return (
    <BoardsScreen
      repository={repository}
      invitations={invitations}
      onOpen={(id) => router.push({ pathname: '/board/[id]', params: { id } })}
      onOpenLab={() => router.push('/board-lab')}
    />
  );
}
