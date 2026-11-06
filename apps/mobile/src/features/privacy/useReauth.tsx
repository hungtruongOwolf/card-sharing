import { useCallback, useRef, useState, type ReactElement } from 'react';

import { t, type MessageKey } from '../../i18n';
import { Button, Sheet, Text } from '../../ui';
import type { IdTokenProvider } from '../auth';
import type { SignInProvider } from '../onboarding/signInProviders';
import { PrivacyError, type PrivacyApi, type ReauthGrant } from './privacyApi';
import { sessionEnded } from './privacyMessages';

const LABELS: Record<IdTokenProvider, MessageKey> = {
  apple: 'reauth.apple',
  google: 'reauth.google',
};

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

/**
 * Re-authentication before a sensitive action (delete account, export data; SR-2). The user picks the
 * provider they signed in with, its native sheet returns a fresh ID token, and the server exchanges it for a
 * re-auth token only if it belongs to this account. The server is the authority: a proof of another account
 * is refused there, never judged here.
 */
export function useReauth(api: PrivacyApi, providers: readonly SignInProvider[]): Reauth {
  const [options, setOptions] = useState<ReauthOptions | null>(null);
  const [busy, setBusy] = useState<IdTokenProvider | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const resolver = useRef<((grant: ReauthGrant | null) => void) | null>(null);

  const finish = useCallback((grant: ReauthGrant | null) => {
    resolver.current?.(grant);
    resolver.current = null;
    setOptions(null);
    setBusy(null);
    setMessage(null);
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
      if (sessionEnded(error)) {
        finish(null);
        return;
      }
      setMessage(
        error instanceof PrivacyError && error.kind === 'proof_rejected'
          ? t('reauth.rejected')
          : error instanceof PrivacyError && error.kind === 'rate_limited'
            ? t('reauth.rateLimited')
            : t('reauth.failed'),
      );
    } finally {
      setBusy((current) => (current === provider.id ? null : current));
    }
  }

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
      {message ? (
        <Text tone="danger" accessibilityRole="alert" accessibilityLiveRegion="polite">
          {message}
        </Text>
      ) : null}
    </Sheet>
  );

  return { request, sheet };
}
