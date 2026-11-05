import { useRouter } from 'expo-router';

import { FriendsScreen, useFriendsApi } from '../../features/friends';

/** Friends list: remove or block a friend (US-6, AC-26.2). */
export default function FriendsRoute() {
  const router = useRouter();
  const api = useFriendsApi();
  return (
    <FriendsScreen
      api={api}
      onOpenRequests={() => router.push('/friends/requests')}
      onOpenInvite={() => router.push('/friends/invite')}
      onOpenFind={() => router.push('/friends/find')}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
    />
  );
}
