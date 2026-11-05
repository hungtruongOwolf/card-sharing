import { useState } from 'react';

import { t } from '../../../i18n';
import { Button, Screen, TextField } from '../../../ui';
import { OnboardingError } from '../onboardingApi';
import { useOnboarding } from '../OnboardingProvider';
import { HANDLE_MAX, HANDLE_MIN, isValidHandle, normalizeHandle } from '../validation';
import { genericErrorMessage } from './errorMessage';
import { FormMessage } from './FormMessage';

/** Unique handle (AC-3.3): 3–20 of `[a-z0-9._]`, case-insensitive; uniqueness and profanity are server checks. */
export function HandleScreen() {
  const { claimHandle } = useOnboarding();
  const [handle, setHandle] = useState('');
  const [touched, setTouched] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const valid = isValidHandle(handle);
  const fieldError =
    touched && !valid
      ? t('onboarding.handle.invalid', { min: HANDLE_MIN, max: HANDLE_MAX })
      : undefined;

  async function submit() {
    setTouched(true);
    if (!valid) return;
    setSubmitting(true);
    setMessage(null);
    try {
      await claimHandle(handle);
    } catch (err) {
      const kind = err instanceof OnboardingError ? err.kind : null;
      setMessage(
        kind === 'taken'
          ? t('onboarding.handle.taken')
          : kind === 'invalid'
            ? t('onboarding.handle.rejected')
            : genericErrorMessage(err),
      );
      setSubmitting(false);
    }
  }

  return (
    <Screen title={t('onboarding.handle.title')} testID="onboarding-handle">
      <TextField
        testID="handle"
        label={t('onboarding.handle.label')}
        hint={t('onboarding.handle.hint', { min: HANDLE_MIN, max: HANDLE_MAX })}
        error={fieldError}
        value={handle}
        onChangeText={(text) => {
          setHandle(normalizeHandle(text));
          setMessage(null);
        }}
        onBlur={() => setTouched(handle.length > 0)}
        autoCapitalize="none"
        autoCorrect={false}
        spellCheck={false}
        autoComplete="username-new"
        textContentType="username"
        maxLength={HANDLE_MAX}
        returnKeyType="done"
        onSubmitEditing={() => void submit()}
        editable={!submitting}
      />
      <FormMessage message={message} />
      <Button
        block
        label={t('onboarding.continue')}
        disabled={handle.length === 0}
        loading={submitting}
        onPress={() => void submit()}
      />
    </Screen>
  );
}
