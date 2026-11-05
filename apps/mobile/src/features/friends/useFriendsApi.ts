import { useMemo } from 'react';

import { useAuthClient } from '../auth';
import { createFriendsApi, type FriendsApi } from './friendsApi';

/** The social API over the signed-in auth client (one instance per auth client). */
export function useFriendsApi(): FriendsApi {
  const authClient = useAuthClient();
  return useMemo(() => createFriendsApi(authClient.fetch), [authClient]);
}
