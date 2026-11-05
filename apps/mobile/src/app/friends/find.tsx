import { useRouter } from 'expo-router';

import { FindFriendScreen, useFriendsApi } from '../../features/friends';

/** Find a friend by exact handle (US-5a). */
export default function FindFriendRoute() {
  const router = useRouter();
  const api = useFriendsApi();
  return (
    <FindFriendScreen
      api={api}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/friends'))}
    />
  );
}
