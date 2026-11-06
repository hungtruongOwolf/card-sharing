import { useMemo } from 'react';

import { useAuthClient } from '../auth';
import { createNotificationsApi, type NotificationsApi } from './notificationsApi';

/** The notification-preference calls over the signed-in auth client (one instance per auth client). */
export function useNotificationsApi(): NotificationsApi {
  const authClient = useAuthClient();
  return useMemo(() => createNotificationsApi(authClient.fetch), [authClient]);
}
