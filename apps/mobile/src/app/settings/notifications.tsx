import { useRouter } from 'expo-router';

import { NotificationSettingsScreen, useNotificationsApi } from '../../features/notifications';

/** Settings → Notifications (T-87, AC-31.1): alert types and quiet hours. */
export default function NotificationSettingsRoute() {
  const router = useRouter();
  const api = useNotificationsApi();
  return (
    <NotificationSettingsScreen
      api={api}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
    />
  );
}
