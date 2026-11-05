import { useRouter } from 'expo-router';

import { InviteScreen, useFriendsApi } from '../../features/friends';

/** My invite link: QR code, share sheet, reset (US-4). */
export default function InviteRoute() {
  const router = useRouter();
  const api = useFriendsApi();
  return (
    <InviteScreen
      api={api}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/friends'))}
    />
  );
}
