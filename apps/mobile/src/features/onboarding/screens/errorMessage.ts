import { t } from '../../../i18n';
import { OnboardingError } from '../onboardingApi';

/** Generic copy for failures a screen has no specific message for. Never echoes server detail. */
export function genericErrorMessage(err: unknown): string {
  if (err instanceof OnboardingError && err.kind === 'rate_limited') {
    return t('onboarding.error.rateLimited');
  }
  return t('onboarding.error.generic');
}
