import { useState } from 'react';
import { StyleSheet, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t } from '../../../i18n';
import { Button, Text, TextField } from '../../../ui';
import { isPlausibleEmail, MagicLinkStartError, type MagicLinkSignIn } from '../magicLink';
import { FormMessage } from './FormMessage';

interface Props {
  magicLink: MagicLinkSignIn;
  /** Sign-up invite code from the link that opened the screen (`?invite=`), kept with the request. */
  inviteCode?: string;
  /** The in-app link the user started from, opened once they are signed in (in the tab the email link opens). */
  returnTo: string | null;
}

function startErrorMessage(error: unknown): string {
  if (error instanceof MagicLinkStartError) {
    switch (error.kind) {
      case 'invalid_email':
        return t('onboarding.signIn.emailInvalid');
      case 'rate_limited':
        return t('onboarding.signIn.emailRateLimited');
      case 'busy':
        return t('error.serverBusy');
      case 'not_supported':
        return t('onboarding.signIn.emailUnsupported');
      case 'unavailable':
        break;
    }
  }
  return t('onboarding.signIn.emailFailed');
}

/**
 * "Email me a sign-in link" (US-1, ADR-0012): asks for a magic link, then says where to open it. The answer is the
 * same whether or not the address has an account (AC-1.2); the link signs in, or signs up behind the invite gate.
 */
export function EmailSignInSection({ magicLink, inviteCode, returnTo }: Props) {
  const [email, setEmail] = useState('');
  const [touched, setTouched] = useState(false);
  const [sentTo, setSentTo] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const address = email.trim();
  const valid = isPlausibleEmail(address);

  async function send() {
    setTouched(true);
    setMessage(null);
    if (!valid) return;
    setBusy(true);
    try {
      await magicLink.start(address, { inviteCode, returnTo: returnTo ?? undefined });
      setSentTo(address);
    } catch (error) {
      setMessage(startErrorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  if (sentTo !== null) {
    return (
      <View testID="check-email" style={styles.box}>
        <Text variant="title.md" accessibilityRole="header">
          {t('onboarding.checkEmail.title')}
        </Text>
        <Text>{t('onboarding.checkEmail.body', { email: sentTo })}</Text>
        <Text tone="muted">{t('onboarding.checkEmail.spam')}</Text>
        <Button
          block
          variant="secondary"
          label={t('onboarding.checkEmail.resend')}
          loading={busy}
          onPress={() => void send()}
        />
        <Button
          block
          variant="ghost"
          label={t('onboarding.checkEmail.change')}
          disabled={busy}
          onPress={() => {
            setSentTo(null);
            setMessage(null);
          }}
        />
        <FormMessage message={message} />
      </View>
    );
  }

  return (
    <View testID="email-sign-in" style={styles.box}>
      <TextField
        testID="sign-in-email"
        label={t('onboarding.signIn.emailLabel')}
        hint={t('onboarding.signIn.emailHint')}
        error={
          touched && address.length > 0 && !valid ? t('onboarding.signIn.emailInvalid') : undefined
        }
        value={email}
        onChangeText={(text) => {
          setEmail(text);
          setMessage(null);
        }}
        onBlur={() => setTouched(address.length > 0)}
        autoCapitalize="none"
        autoCorrect={false}
        autoComplete="email"
        inputMode="email"
        keyboardType="email-address"
        textContentType="emailAddress"
        returnKeyType="send"
        maxLength={320}
        onSubmitEditing={() => void send()}
        editable={!busy}
      />
      {inviteCode ? <Text tone="muted">{t('onboarding.signIn.inviteReady')}</Text> : null}
      <Button
        block
        variant="primary"
        label={t('onboarding.signIn.emailSend')}
        loading={busy}
        onPress={() => void send()}
      />
      <FormMessage message={message} />
    </View>
  );
}

const styles = StyleSheet.create({
  box: { gap: space[3] },
});
