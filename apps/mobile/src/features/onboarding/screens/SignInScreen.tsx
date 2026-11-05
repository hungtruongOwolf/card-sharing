import { useState } from 'react';

import { t, type MessageKey } from '../../../i18n';
import { Button, Screen, Text } from '../../../ui';
import { useAuthClient, type IdTokenProvider } from '../../auth';
import { useOnboarding } from '../OnboardingProvider';
import type { SignInProvider } from '../signInProviders';
import { FormMessage } from './FormMessage';

const LABELS: Record<IdTokenProvider, MessageKey> = {
  apple: 'onboarding.signIn.apple',
  google: 'onboarding.signIn.google',
};

/**
 * First screen (US-1). Each button asks its provider for an ID token and hands it to the auth client's
 * `signInWithIdToken` seam; the server creates the account and the flow continues from `/v1/me`.
 */
export function SignInScreen() {
  const { signInProviders } = useOnboarding();
  const authClient = useAuthClient();
  const [busy, setBusy] = useState<IdTokenProvider | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  async function signIn(provider: SignInProvider) {
    setMessage(null);
    if (!provider.isAvailable()) {
      setMessage(t('onboarding.signIn.unavailable'));
      return;
    }
    setBusy(provider.id);
    try {
      const credential = await provider.getCredential();
      if (!credential) return; // cancelled by the user
      await authClient.signInWithIdToken(provider.id, credential.idToken, credential.nonce, {
        authorizationCode: credential.authorizationCode,
      });
    } catch {
      setMessage(t('onboarding.signIn.failed'));
    } finally {
      setBusy(null);
    }
  }

  return (
    <Screen title={t('onboarding.signIn.title')} testID="onboarding-sign-in">
      <Text tone="muted">{t('onboarding.signIn.body')}</Text>
      {signInProviders.map((provider) => (
        <Button
          key={provider.id}
          block
          variant={provider.id === 'apple' ? 'primary' : 'secondary'}
          label={t(LABELS[provider.id])}
          loading={busy === provider.id}
          disabled={busy !== null && busy !== provider.id}
          onPress={() => void signIn(provider)}
        />
      ))}
      <FormMessage message={message} />
    </Screen>
  );
}
