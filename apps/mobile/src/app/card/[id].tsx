import { useLocalSearchParams, useRouter } from 'expo-router';

import { CardViewerScreen } from '../../features/feed';
import { useRemoteConfig } from '../../features/remote-config';
import { useReportSheet } from '../../features/reports';
import { useCardsApi } from '../../features/sharing';

/** One card full screen with its finish and squish (T-81, US-15); opened from the feed. */
export default function CardRoute() {
  const router = useRouter();
  const api = useCardsApi();
  const remoteConfig = useRemoteConfig();
  const { id } = useLocalSearchParams<{ id: string }>();
  const report = useReportSheet();
  return (
    <>
      <CardViewerScreen
        api={api}
        cardId={typeof id === 'string' ? id : ''}
        remoteConfig={remoteConfig?.registry}
        onReport={(cardId) => report.open({ kind: 'card', id: cardId })}
        onClose={() => (router.canGoBack() ? router.back() : router.replace('/feed'))}
      />
      {report.sheet}
    </>
  );
}
