import { useFocusEffect, useRouter } from 'expo-router';
import { useCallback, useState } from 'react';

import { deviceDraftStore, DraftsScreen } from '../features/drafts';

/** Private drafts tray (T-68). Opening a draft reopens it in the editor; Send goes to the send screen (T-77). */
export default function DraftsRoute() {
  const router = useRouter();
  // Remount on focus so coming back from the editor shows the latest auto-saved changes.
  const [visit, setVisit] = useState(0);
  useFocusEffect(useCallback(() => setVisit((n) => n + 1), []));
  return (
    <DraftsScreen
      key={visit}
      store={deviceDraftStore()}
      onOpen={(card) => router.push({ pathname: '/editor', params: { draft: card.id } })}
      onSend={(card) => router.push({ pathname: '/send', params: { draft: card.id } })}
      onNew={() => router.push('/capture')}
    />
  );
}
