import { useState } from 'react';
import { StyleSheet } from 'react-native';

import { t, type MessageKey } from '../../../i18n';
import { Button, Screen, Text, TextField } from '../../../ui';
import { isServerBusy } from '../../../data/serverBusy';
import { AuthError, useAuthClient, type IdTokenProvider } from '../../auth';
import { GoogleWebButton, type GoogleCredential } from '../../auth/GoogleWebButton';
import { googleWebClientId, type GisApi } from '../../auth/googleIdentity';
import { usePendingLink } from '../GuardedStack';
import { useOnboarding } from '../OnboardingProvider';
import type { SignInProvider } from '../signInProviders';
import { DevSignInSection } from './DevSignInSection';
import { EmailSignInSection } from './EmailSignInSection';
import { FormMessage } from './FormMessage';

const LABELS: Record<IdTokenProvider, MessageKey> = {
  apple: 'onboarding.signIn.apple',
  google: 'onboarding.signIn.google',
};

/** A sign-up code as it may arrive in `?invite=` (the server judges it; this only keeps junk out). */
const INVITE_PARAM = /^[A-Za-z0-9 -]{1,32}$/;

export interface SignInScreenProps {
  /** `?invite=<code>` of the sign-up link staff send (ADR-0012); used if the sign-in creates an account. */
  inviteCode?: string;
  /** Google's web button config; defaults to `EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID` on the web (tests inject one). */
  googleWeb?: { clientId: string; loadGis: () => Promise<GisApi> } | null;
}

const isInviteRefusal = (error: unknown) =>
  error instanceof AuthError &&
  (error.code === 'INVITE_REQUIRED' || error.code === 'INVITE_INVALID');

/**
 * First screen (US-1). Email magic link first (ADR-0012), then the provider buttons: each asks its provider for an
 * ID token and hands it to the auth client's `signInWithIdToken` seam; the server creates the account and the flow
 * continues from `/v1/me`.
 */
export function SignInScreen({ inviteCode, googleWeb }: SignInScreenProps) {
  const { signInProviders, devSignIn, magicLink } = useOnboarding();
  const pendingLink = usePendingLink();
  const invite = inviteCode && INVITE_PARAM.test(inviteCode) ? inviteCode : undefined;
  const authClient = useAuthClient();
  const [busy, setBusy] = useState<IdTokenProvider | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const googleClientId =
    googleWeb === undefined ? googleWebClientId() : (googleWeb?.clientId ?? null);
  /** A Google credential refused for a missing or wrong invite code: retried as is with the typed code. */
  const [googleWaiting, setGoogleWaiting] = useState<GoogleCredential | null>(null);
  const [googleCode, setGoogleCode] = useState(invite ?? '');

  async function signInWithGoogle(credential: GoogleCredential, code = invite) {
    setMessage(null);
    setBusy('google');
    try {
      await authClient.signInWithIdToken('google', credential.idToken, credential.nonce, {
        inviteCode: code,
      });
    } catch (error) {
      if (isInviteRefusal(error)) {
        // The server did not use up the token's nonce: the same credential goes again with a code.
        setGoogleWaiting(credential);
        if (error instanceof AuthError && error.code === 'INVITE_INVALID') {
          setMessage(t('magicLink.inviteInvalid'));
        }
      } else {
        setMessage(isServerBusy(error) ? t('error.serverBusy') : t('onboarding.signIn.failed'));
      }
    } finally {
      setBusy(null);
    }
  }

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
        inviteCode: invite,
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
      {magicLink ? (
        <>
          <EmailSignInSection
            magicLink={magicLink}
            inviteCode={invite}
            returnTo={pendingLink.href}
          />
          <Text tone="muted" style={styles.or}>
            {t('onboarding.signIn.or')}
          </Text>
        </>
      ) : null}
      {googleClientId && googleWaiting ? (
        <>
          <Text variant="title.md" accessibilityRole="header">
            {t('magicLink.inviteTitle')}
          </Text>
          <Text tone="muted">{t('magicLink.inviteBody')}</Text>
          <TextField
            testID="google-invite-code"
            label={t('magicLink.inviteLabel')}
            hint={t('magicLink.inviteHint')}
            value={googleCode}
            onChangeText={(text) => {
              setGoogleCode(text);
              setMessage(null);
            }}
            autoCapitalize="characters"
            autoCorrect={false}
            autoComplete="off"
            maxLength={32}
            editable={busy === null}
          />
          <Button
            block
            variant="primary"
            label={t('magicLink.inviteJoin')}
            loading={busy === 'google'}
            onPress={() => {
              const typed = googleCode.trim();
              if (!typed) setMessage(t('magicLink.inviteMissing'));
              else void signInWithGoogle(googleWaiting, typed);
            }}
          />
        </>
      ) : googleClientId ? (
        <GoogleWebButton
          clientId={googleClientId}
          context="signin"
          loadGis={googleWeb?.loadGis}
          onCredential={(credential) => void signInWithGoogle(credential)}
        />
      ) : null}
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
      {/* Development builds only (#306): `__DEV__` is false in release builds, so this entry is compiled out. */}
      {__DEV__ && devSignIn ? <DevSignInSection devSignIn={devSignIn} /> : null}
    </Screen>
  );
}

const styles = StyleSheet.create({
  or: { textAlign: 'center' },
});
