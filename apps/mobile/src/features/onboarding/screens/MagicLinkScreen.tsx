import { useEffect, useState } from 'react';

import { t, type MessageKey } from '../../../i18n';
import { Button, Screen, Text, TextField } from '../../../ui';
import type { LinkFragment, LinkOutcome, MagicLinkSignIn, PendingMagicLink } from '../magicLink';
import { FormMessage } from './FormMessage';

type View =
  'reading' | 'confirm' | 'invite' | 'expired' | 'used' | 'otherBrowser' | 'badLink' | 'suspended';

/** Final states: the link cannot be used any more; the user asks for a new one. */
const FINAL: Partial<Record<View, readonly [heading: MessageKey, text: MessageKey]>> = {
  expired: ['magicLink.expiredTitle', 'magicLink.expiredBody'],
  used: ['magicLink.usedTitle', 'magicLink.usedBody'],
  otherBrowser: ['magicLink.otherBrowserTitle', 'magicLink.otherBrowserBody'],
  badLink: ['magicLink.badLinkTitle', 'magicLink.badLinkBody'],
  suspended: ['magicLink.suspendedTitle', 'magicLink.suspended'],
};

interface Props {
  /** The link this screen was opened with; `undefined` while it is read, null when there is none. */
  link: LinkFragment | null | undefined;
  magicLink: MagicLinkSignIn | null;
  /** Remembers the in-app link the user started from, so the app opens it once signed in. */
  onContinueAt(href: string): void;
  /** Back to the sign-in screen to ask for a new link. */
  onRestart(): void;
}

/**
 * Landing screen of an emailed sign-in link (`/auth/link#rid=...&token=...`, T-23, ADR-0012). It signs in only on
 * the device and browser that asked for the link (the PKCE verifier kept under the request id) and only after the
 * user confirms (AC-1.3: opening the link never consumes it). When the link would create an account and the sign-up
 * gate needs a code, it asks for one (prefilled from the invite link) and retries the same link.
 */
export function MagicLinkScreen({ link, magicLink, onContinueAt, onRestart }: Props) {
  const [view, setView] = useState<View>('reading');
  const [pending, setPending] = useState<PendingMagicLink | null>(null);
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    if (!link || !magicLink) return;
    let cancelled = false;
    void magicLink.lookup(link.requestId).then(
      (found) => {
        if (cancelled) return;
        if (found.status === 'found') {
          setPending(found.link);
          setCode(found.link.inviteCode ?? '');
          setView('confirm');
        } else {
          setView(found.status === 'expired' ? 'expired' : 'otherBrowser');
        }
      },
      () => {
        if (!cancelled) setView('otherBrowser');
      },
    );
    return () => {
      cancelled = true;
    };
  }, [link, magicLink]);

  async function redeem(inviteCode?: string) {
    if (!link || !magicLink) return;
    setBusy(true);
    setMessage(null);
    // Before the sign-in: once it lands, the phase moves on and this screen goes away.
    if (pending?.returnTo) onContinueAt(pending.returnTo);
    const outcome: LinkOutcome = await magicLink.complete(link.requestId, link.token, inviteCode);
    switch (outcome) {
      case 'signed_in':
        return; // The onboarding phase takes over.
      case 'invite_required':
        setView('invite');
        break;
      case 'invite_invalid':
        setView('invite');
        setMessage(t('magicLink.inviteInvalid'));
        break;
      case 'expired':
      case 'used':
      case 'suspended':
        setView(outcome);
        break;
      case 'invalid':
        setView('otherBrowser');
        break;
      case 'rate_limited':
        setMessage(t('magicLink.rateLimited'));
        break;
      case 'unavailable':
        setMessage(t('magicLink.failed'));
        break;
    }
    setBusy(false);
  }

  /** Sends the typed invite code with the same link. */
  function join() {
    const typed = code.trim();
    if (!typed) {
      setMessage(t('magicLink.inviteMissing'));
      return Promise.resolve();
    }
    return redeem(typed);
  }

  // No usable link (or nothing to redeem it with) is final from the start; otherwise the lookup decides.
  const shown: View = link === undefined ? 'reading' : !link || !magicLink ? 'badLink' : view;
  const final = FINAL[shown];
  if (final) {
    const [heading, text] = final;
    return (
      <Screen title={t(heading)} testID={`magic-link-${shown}`}>
        <Text>{t(text)}</Text>
        <Button block variant="primary" label={t('magicLink.restart')} onPress={onRestart} />
      </Screen>
    );
  }

  if (shown === 'invite') {
    return (
      <Screen title={t('magicLink.inviteTitle')} testID="magic-link-invite">
        <Text tone="muted">{t('magicLink.inviteBody')}</Text>
        <TextField
          testID="invite-code"
          label={t('magicLink.inviteLabel')}
          hint={t('magicLink.inviteHint')}
          value={code}
          onChangeText={(text) => {
            setCode(text);
            setMessage(null);
          }}
          autoCapitalize="characters"
          autoCorrect={false}
          autoComplete="off"
          maxLength={32}
          returnKeyType="go"
          editable={!busy}
          onSubmitEditing={() => void join()}
        />
        <Button
          block
          variant="primary"
          label={t('magicLink.inviteJoin')}
          loading={busy}
          onPress={() => void join()}
        />
        <FormMessage message={message} />
      </Screen>
    );
  }

  if (shown === 'confirm') {
    return (
      <Screen title={t('magicLink.title')} testID="magic-link-confirm">
        <Text tone="muted">{t('magicLink.body')}</Text>
        <Button
          block
          variant="primary"
          label={t('magicLink.continue')}
          loading={busy}
          onPress={() => void redeem()}
        />
        <FormMessage message={message} />
      </Screen>
    );
  }

  return (
    <Screen title={t('magicLink.title')} testID="magic-link-reading">
      <Text tone="muted">{t('magicLink.reading')}</Text>
    </Screen>
  );
}
