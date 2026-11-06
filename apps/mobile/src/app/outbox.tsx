import { useRouter } from 'expo-router';

import { OutboxScreen, useOutbox } from '../features/outbox';
import { t } from '../i18n';
import { Screen, Text } from '../ui';

/** Sends and reactions waiting for the network, with cancel / retry / delete (T-78). */
export default function OutboxRoute() {
  const router = useRouter();
  const context = useOutbox();
  if (!context) {
    return (
      <Screen centered>
        <Text tone="muted">{t('outbox.empty')}</Text>
      </Screen>
    );
  }
  return (
    <OutboxScreen
      outbox={context.outbox}
      entries={context.entries}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/'))}
    />
  );
}
