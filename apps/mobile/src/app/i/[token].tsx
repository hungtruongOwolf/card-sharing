import { useLocalSearchParams, useRouter } from 'expo-router';

import { InvitePreviewScreen, useFriendsApi } from '../../features/friends';

/**
 * An invite link opened in the app (`cardsharing://i/<token>`; universal links come with T-49). The token is
 * validated by the screen before any request; the route itself trusts nothing in it.
 */
export default function InviteLinkRoute() {
  const router = useRouter();
  const api = useFriendsApi();
  const { token } = useLocalSearchParams<{ token?: string | string[] }>();
  return (
    <InvitePreviewScreen
      api={api}
      token={typeof token === 'string' ? token : ''}
      onDone={() => router.replace('/friends')}
    />
  );
}
