import { useRouter } from 'expo-router';

import { FriendsScreen, useFriendsApi } from '../../features/friends';
import { useReportSheet } from '../../features/reports';

/** Friends list: remove or block a friend (US-6, AC-26.2). */
export default function FriendsRoute() {
  const router = useRouter();
  const api = useFriendsApi();
  const report = useReportSheet();
  return (
    <>
      <FriendsScreen
        api={api}
        onOpenRequests={() => router.push('/friends/requests')}
        onOpenInvite={() => router.push('/friends/invite')}
        onOpenFind={() => router.push('/friends/find')}
        onReport={(person) => report.open({ kind: 'user', id: person.id })}
        onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
      />
      {report.sheet}
    </>
  );
}
