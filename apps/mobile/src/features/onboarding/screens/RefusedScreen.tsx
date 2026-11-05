import { t } from '../../../i18n';
import { Button, Screen, Text } from '../../../ui';
import { useAuthState } from '../../auth';
import { useOnboarding } from '../OnboardingProvider';

/**
 * Neutral age-gate refusal (AC-2.1, SR-25): no reason, no threshold, no countdown. The server has already
 * deleted the pending account; "Back to start" returns to sign-in once the device session is cleared.
 */
export function RefusedScreen() {
  const { acknowledgeRefusal } = useOnboarding();
  const signingOut = useAuthState().status !== 'signedOut';
  return (
    <Screen title={t('onboarding.refused.title')} centered testID="onboarding-refused">
      <Text tone="muted">{t('onboarding.refused.body')}</Text>
      <Button
        label={t('onboarding.refused.back')}
        loading={signingOut}
        onPress={acknowledgeRefusal}
      />
    </Screen>
  );
}
