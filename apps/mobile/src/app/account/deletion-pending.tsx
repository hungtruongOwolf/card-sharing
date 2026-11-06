import { useMemo } from 'react';

import { useAuthClient } from '../../features/auth';
import { useOnboarding } from '../../features/onboarding';
import { createPrivacyApi, DeletionPendingScreen } from '../../features/privacy';

/** The only screen of a deletion-pending account (AC-29.2): cancel the deletion or sign out. */
export default function DeletionPendingRoute() {
  const authClient = useAuthClient();
  const { refreshAccount } = useOnboarding();
  const api = useMemo(() => createPrivacyApi(authClient), [authClient]);
  return (
    <DeletionPendingScreen
      api={api}
      onAccountChanged={refreshAccount}
      signOut={authClient.signOut}
    />
  );
}
