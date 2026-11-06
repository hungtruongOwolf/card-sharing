import { useRouter } from 'expo-router';

import { BoardLabScreen } from '../features/boards';

/** Spike #182 board canvas lab; reachable from the boards list. */
export default function BoardLabRoute() {
  const router = useRouter();
  return <BoardLabScreen onClose={() => router.back()} />;
}
