import { useRouter } from 'expo-router';
import { useMemo } from 'react';

import { useAuthClient } from '../../features/auth';
import { createSessionsApi, SessionsScreen } from '../../features/sessions';

/** Settings → signed-in devices (AC-1.4). After "Log out everywhere" the phase guard moves to sign-in. */
export default function SessionsRoute() {
  const router = useRouter();
  const authClient = useAuthClient();
  const api = useMemo(() => createSessionsApi(authClient.fetch), [authClient]);
  return (
    <SessionsScreen
      api={api}
      signOutEverywhere={authClient.signOutEverywhere}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
    />
  );
}
