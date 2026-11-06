import { useMemo } from 'react';

import { useAuthClient } from '../auth';
import { createActivityApi, type ActivityApi } from './activityApi';

/** The activity calls over the signed-in auth client (one instance per auth client). */
export function useActivityApi(): ActivityApi {
  const authClient = useAuthClient();
  return useMemo(() => createActivityApi(authClient.fetch), [authClient]);
}
