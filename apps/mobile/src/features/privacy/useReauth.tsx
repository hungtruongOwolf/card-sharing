import { useCallback, useEffect, useRef, useState, type ReactElement } from 'react';

import { t, type MessageKey } from '../../i18n';
import { Button, Sheet, Text, TextField } from '../../ui';
import type { IdTokenProvider } from '../auth';
import { appMagicLinkChannel, type MagicLinkChannel } from '../auth/magicLinkChannel';
import { createPkcePair, platformRandom } from '../auth/pkce';
import type { SignInProvider } from '../onboarding/signInProviders';
import { PrivacyError, type PrivacyApi, type ReauthGrant } from './privacyApi';
import { sessionEnded } from './privacyMessages';

const LABELS: Record<IdTokenProvider, MessageKey> = {
  apple: 'reauth.apple',
  google: 'reauth.google',
};

/** Loose shape check before asking the server, which validates for real. */
const EMAIL = /^[^\s@]{1,64}@[^\s@]+\.[^\s@]+$/;

export interface ReauthOptions {
  /** Sheet title from `t()`, naming what the confirmation is for. */
  title: string;
  /** One line under the title. */
  body: string;
}

export interface Reauth {
  /**
   * Opens the "confirm it's you" sheet. Resolves with a fresh 5-minute re-auth token (T-26), or null when the
   * user closes the sheet. The token lives only in memory for the one action that asked for it.
   */
  request(options: ReauthOptions): Promise<ReauthGrant | null>;
  /** The sheet element; render it once in the screen. */
  sheet: ReactElement;
}

export interface ReauthDeps {
  /** Where the tab a re-auth link opens in hands it over (ADR-0012). */
  channel?: MagicLinkChannel;
  /** CSPRNG for the link's PKCE verifier; null = email confirmation unavailable here. */
  random?: ((bytes: Uint8Array) => Uint8Array) | null;
}

/** The re-auth link this sheet asked for: its request id and the verifier, kept in memory only. */
interface PendingLink {
  readonly requestId: string;
  readonly verifier: string;
  readonly email: string;
}

/**
 * Re-authentication before a sensitive action (delete account, export data; SR-2). The user picks how they signed
 * in: a provider's native sheet returns a fresh ID token, or (accounts that signed up by email, ADR-0012) a re-auth
 * link is emailed to the address they type. The link opens in another tab, which hands its token to this sheet over
 * the magic-link channel; the sheet redeems it with the verifier only it holds. Either way the server exchanges the
 * proof for a re-auth token only if it belongs to this account and session: the server is the authority.
 */
export function useReauth(
  api: PrivacyApi,
  providers: readonly SignInProvider[],
  { channel = appMagicLinkChannel(), random = platformRandom() }: ReauthDeps = {},
): Reauth {
  const [options, setOptions] = useState<ReauthOptions | null>(null);
  const [busy, setBusy] = useState<IdTokenProvider | 'email' | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [emailOpen, setEmailOpen] = useState(false);
  const [email, setEmail] = useState('');
  const [pending, setPending] = useState<PendingLink | null>(null);
  const resolver = useRef<((grant: ReauthGrant | null) => void) | null>(null);

  const finish = useCallback((grant: ReauthGrant | null) => {
    resolver.current?.(grant);
    resolver.current = null;
    setOptions(null);
    setBusy(null);
    setMessage(null);
    setEmailOpen(false);
    setPending(null);
  }, []);

  const request = useCallback(
    (next: ReauthOptions) =>
      new Promise<ReauthGrant | null>((resolve) => {
        resolver.current?.(null); // a second request replaces an open one
        resolver.current = resolve;
        setMessage(null);
        setOptions(next);
      }),
    [],
  );

  const failed = useCallback(
    (error: unknown) => {
      if (sessionEnded(error)) {
        finish(null);
        return;
      }
      const kind = error instanceof PrivacyError ? error.kind : null;
      setMessage(
        kind === 'proof_rejected'
          ? t('reauth.rejected')
          : kind === 'rate_limited'
            ? t('reauth.rateLimited')
            : kind === 'link_spent'
              ? t('reauth.linkSpent')
              : kind === 'link_invalid'
                ? t('reauth.linkInvalid')
                : kind === 'invalid_email'
                  ? t('reauth.emailInvalid')
                  : t('reauth.failed'),
      );
    },
    [finish],
  );

  // While a re-auth link is out, wait for the tab it opens in to hand over its token.
  useEffect(() => {
    if (!pending) return;
    return channel.subscribe((received) => {
      if (received.type !== 'link' || received.requestId !== pending.requestId) return;
      channel.post({ type: 'ack', requestId: pending.requestId });
      setBusy('email');
      setMessage(null);
      api.verifyEmailReauth(received.token, pending.verifier).then(
        (grant) => finish(grant),
        (error: unknown) => {
          setBusy(null);
          setPending(null);
          failed(error);
        },
      );
    });
  }, [api, channel, failed, finish, pending]);

  async function confirm(provider: SignInProvider) {
    setMessage(null);
    if (!provider.isAvailable()) {
      setMessage(t('reauth.unavailable'));
      return;
    }
    setBusy(provider.id);
    try {
      const credential = await provider.getCredential();
      if (!credential) return; // cancelled in the provider sheet
      const grant = await api.reauthenticate({
        provider: provider.id,
        idToken: credential.idToken,
        nonce: credential.nonce,
      });
      finish(grant);
    } catch (error) {
      failed(error);
    } finally {
      setBusy((current) => (current === provider.id ? null : current));
    }
  }

  async function sendLink() {
    setMessage(null);
    const address = email.trim();
    if (!EMAIL.test(address)) {
      setMessage(t('reauth.emailInvalid'));
      return;
    }
    if (!random) {
      setMessage(t('reauth.unavailable'));
      return;
    }
    setBusy('email');
    try {
      const pair = createPkcePair(random);
      const requestId = await api.startEmailReauth(address, pair.challenge);
      setPending({ requestId, verifier: pair.verifier, email: address });
    } catch (error) {
      failed(error);
    } finally {
      setBusy((current) => (current === 'email' ? null : current));
    }
  }

  const emailSection = pending ? (
    <>
      <Text testID="reauth-email-sent">{t('reauth.emailSent', { email: pending.email })}</Text>
      <Button
        block
        variant="ghost"
        label={t('reauth.emailResend')}
        disabled={busy !== null}
        onPress={() => {
          setPending(null);
          setMessage(null);
        }}
      />
    </>
  ) : emailOpen ? (
    <>
      <TextField
        testID="reauth-email"
        label={t('reauth.emailLabel')}
        hint={t('reauth.emailHint')}
        value={email}
        onChangeText={(text) => {
          setEmail(text);
          setMessage(null);
        }}
        autoCapitalize="none"
        autoCorrect={false}
        autoComplete="email"
        inputMode="email"
        keyboardType="email-address"
        textContentType="emailAddress"
        maxLength={320}
        editable={busy === null}
        onSubmitEditing={() => void sendLink()}
      />
      <Button
        block
        variant="secondary"
        label={t('reauth.emailSend')}
        loading={busy === 'email'}
        disabled={busy !== null && busy !== 'email'}
        onPress={() => void sendLink()}
      />
    </>
  ) : (
    <Button
      block
      variant="secondary"
      label={t('reauth.email')}
      disabled={busy !== null}
      onPress={() => {
        setEmailOpen(true);
        setMessage(null);
      }}
    />
  );

  const sheet = (
    <Sheet
      visible={options !== null}
      onClose={() => {
        if (busy === null) finish(null);
      }}
      title={options?.title ?? ''}
      testID="reauth-sheet"
    >
      <Text tone="muted">{options?.body}</Text>
      {providers.map((provider) => (
        <Button
          key={provider.id}
          block
          variant={provider.id === 'apple' ? 'primary' : 'secondary'}
          label={t(LABELS[provider.id])}
          loading={busy === provider.id}
          disabled={busy !== null && busy !== provider.id}
          onPress={() => void confirm(provider)}
        />
      ))}
      {emailSection}
      {message ? (
        <Text tone="danger" accessibilityRole="alert" accessibilityLiveRegion="polite">
          {message}
        </Text>
      ) : null}
    </Sheet>
  );

  return { request, sheet };
}
