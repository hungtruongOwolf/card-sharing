import { useRouter } from 'expo-router';
import { useMemo } from 'react';

import { useAuthClient } from '../../features/auth';
import { useOnboarding } from '../../features/onboarding';
import { createPrivacyApi, DeleteAccountScreen, formatDateTime } from '../../features/privacy';
import { t } from '../../i18n';
import { useToast } from '../../ui';

/**
 * Settings → Delete account (US-29). The server has already logged out every device when it accepts; signing
 * out here clears this device (session, feed, outbox, boards and the widget token, AC-16.2) and the phase guard
 * moves to sign-in.
 */
export default function DeleteAccountRoute() {
  const router = useRouter();
  const toast = useToast();
  const authClient = useAuthClient();
  const { signInProviders } = useOnboarding();
  const api = useMemo(() => createPrivacyApi(authClient), [authClient]);
  return (
    <DeleteAccountScreen
      api={api}
      providers={signInProviders}
      onDeleted={async (deletion) => {
        toast.show({
          message: t('deleteAccount.done', { date: formatDateTime(deletion.graceUntil) }),
        });
        await authClient.signOut();
      }}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
    />
  );
}
