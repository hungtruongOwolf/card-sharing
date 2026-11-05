import { useRouter } from 'expo-router';

import { PrivacyScreen, useFriendsApi } from '../../features/friends';

/** Settings → Privacy: findable by handle, blocked accounts (T-45, T-47). */
export default function PrivacyRoute() {
  const router = useRouter();
  const api = useFriendsApi();
  return (
    <PrivacyScreen
      api={api}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
    />
  );
}
