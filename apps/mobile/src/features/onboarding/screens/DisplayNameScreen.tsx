import { useState } from 'react';

import { t } from '../../../i18n';
import { Button, Screen, TextField } from '../../../ui';
import { OnboardingError } from '../onboardingApi';
import { useOnboarding } from '../OnboardingProvider';
import { DISPLAY_NAME_MAX, DISPLAY_NAME_MIN, displayNameIssue } from '../validation';
import { genericErrorMessage } from './errorMessage';
import { FormMessage } from './FormMessage';

/** Display name (AC-3.2): 2–30 characters, not unique, never searchable. The server runs the profanity filter. */
export function DisplayNameScreen() {
  const { saveDisplayName } = useOnboarding();
  const [name, setName] = useState('');
  const [touched, setTouched] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const issue = displayNameIssue(name);
  const fieldError =
    touched && issue === 'tooShort'
      ? t('onboarding.displayName.tooShort', { min: DISPLAY_NAME_MIN })
      : issue === 'tooLong'
        ? t('onboarding.displayName.tooLong', { max: DISPLAY_NAME_MAX })
        : undefined;

  async function submit() {
    setTouched(true);
    if (issue) return;
    setSubmitting(true);
    setMessage(null);
    try {
      await saveDisplayName(name.trim());
    } catch (err) {
      setMessage(
        err instanceof OnboardingError && err.kind === 'invalid'
          ? t('onboarding.displayName.rejected')
          : genericErrorMessage(err),
      );
      setSubmitting(false);
    }
  }

  return (
    <Screen title={t('onboarding.displayName.title')} testID="onboarding-display-name">
      <TextField
        testID="display-name"
        label={t('onboarding.displayName.label')}
        hint={t('onboarding.displayName.hint', { min: DISPLAY_NAME_MIN, max: DISPLAY_NAME_MAX })}
        error={fieldError}
        value={name}
        onChangeText={(text) => {
          setName(text);
          setMessage(null);
        }}
        onBlur={() => setTouched(name.length > 0)}
        autoComplete="nickname"
        textContentType="nickname"
        returnKeyType="done"
        onSubmitEditing={() => void submit()}
        editable={!submitting}
      />
      <FormMessage message={message} />
      <Button
        block
        label={t('onboarding.continue')}
        disabled={name.trim().length === 0}
        loading={submitting}
        onPress={() => void submit()}
      />
    </Screen>
  );
}
