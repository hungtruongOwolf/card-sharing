import { useRouter } from 'expo-router';

import { ActivityScreen, useActivityApi } from '../features/activity';
import { useFriendsApi } from '../features/friends';
import { useCardsApi } from '../features/sharing';

/** Activity (T-88, AC-31.3): every event, also when no push was sent; a tap opens the card, requests or friends. */
export default function ActivityRoute() {
  const router = useRouter();
  const activity = useActivityApi();
  const cards = useCardsApi();
  const friends = useFriendsApi();
  return (
    <ActivityScreen
      activity={activity}
      cards={cards}
      friends={friends}
      onOpenCard={(id) => router.push({ pathname: '/card/[id]', params: { id } })}
      onOpenRequests={() => router.push('/friends/requests')}
      onOpenFriends={() => router.push('/friends')}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
    />
  );
}
