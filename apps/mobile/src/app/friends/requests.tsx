import { useRouter } from 'expo-router';

import { RequestsScreen, useFriendsApi } from '../../features/friends';

/** Incoming and outgoing friend requests (US-5). */
export default function RequestsRoute() {
  const router = useRouter();
  const api = useFriendsApi();
  return (
    <RequestsScreen
      api={api}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/friends'))}
    />
  );
}
