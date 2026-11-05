import { ActivityIndicator } from 'react-native';

import { t } from '../../../i18n';
import { Button, Screen, Text, useTheme } from '../../../ui';
import { useOnboarding } from '../OnboardingProvider';

/** Shown while the session or `/v1/me` loads. */
export function LoadingScreen() {
  const theme = useTheme();
  return (
    <Screen centered testID="onboarding-loading">
      <ActivityIndicator
        color={theme.color.primary.base}
        accessibilityLabel={t('onboarding.loading')}
      />
    </Screen>
  );
}

/** `/v1/me` failed (offline, 5xx): always offer a next step. */
export function ErrorScreen() {
  const { retry } = useOnboarding();
  return (
    <Screen title={t('onboarding.error.title')} centered>
      <Text tone="muted">{t('onboarding.error.body')}</Text>
      <Button label={t('onboarding.error.retry')} onPress={retry} />
    </Screen>
  );
}
