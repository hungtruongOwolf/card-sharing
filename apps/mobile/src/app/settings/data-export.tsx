import { useRouter } from 'expo-router';
import { useMemo } from 'react';
import { Linking } from 'react-native';

import { useAuthClient } from '../../features/auth';
import { apiBaseUrl } from '../../features/auth/config';
import { useOnboarding } from '../../features/onboarding';
import { createPrivacyApi, DataExportScreen, deviceExportStore } from '../../features/privacy';

/** Settings → Download your data (US-30). The single-use ZIP link opens in the system browser. */
export default function DataExportRoute() {
  const router = useRouter();
  const authClient = useAuthClient();
  const { signInProviders } = useOnboarding();
  const api = useMemo(() => createPrivacyApi(authClient), [authClient]);
  return (
    <DataExportScreen
      api={api}
      providers={signInProviders}
      store={deviceExportStore()}
      baseUrl={apiBaseUrl}
      openUrl={(url) => Linking.openURL(url)}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
    />
  );
}
