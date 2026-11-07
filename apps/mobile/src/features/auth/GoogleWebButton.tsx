import { useEffect, useRef, useState } from 'react';
import { StyleSheet, View, type ViewInstance } from 'react-native';

import { t } from '../../i18n';
import { Text } from '../../ui';
import { googleNonce, loadGoogleIdentity, type GisApi } from './googleIdentity';

/** What Google's button hands back: the ID token and the raw nonce it was issued for. */
export interface GoogleCredential {
  readonly idToken: string;
  readonly nonce: string;
}

interface Props {
  /** The web OAuth client id (`googleWebClientId()`). */
  clientId: string;
  /** Sign-in screen: `signin`; the "confirm it's you" sheet: `use`. */
  context: 'signin' | 'use';
  onCredential(credential: GoogleCredential): void;
  /** Loads `google.accounts.id` (tests inject a fake). */
  loadGis?: () => Promise<GisApi>;
  random?: ((bytes: Uint8Array) => Uint8Array) | null;
}

const BUTTON_WIDTH = 320;

/**
 * Google's own "Continue with Google" button on the web (ADR-0012). One raw nonce per mounted button: Google signs
 * its SHA-256 into the ID token and the server checks it (single use once accepted; a refusal before that, like a
 * missing invite code, leaves it usable for the retry). Render it only when `googleWebClientId()` is set.
 */
export function GoogleWebButton({
  clientId,
  context,
  onCredential,
  loadGis = loadGoogleIdentity,
  random,
}: Props) {
  const host = useRef<ViewInstance>(null);
  const callback = useRef(onCredential);
  // One nonce per mounted button.
  const [nonce] = useState(() => (random === undefined ? googleNonce() : googleNonce(random)));
  const [loadFailed, setLoadFailed] = useState(false);

  useEffect(() => {
    callback.current = onCredential;
  }, [onCredential]);

  useEffect(() => {
    if (!nonce) return;
    let cancelled = false;
    loadGis().then(
      (gis) => {
        // On the web a View's ref is its DOM element.
        const element = host.current as unknown as HTMLElement | null;
        if (cancelled || !element) return;
        gis.initialize({
          client_id: clientId,
          callback: (response) => {
            if (typeof response.credential === 'string' && response.credential.length > 0) {
              callback.current({ idToken: response.credential, nonce: nonce.raw });
            }
          },
          nonce: nonce.hashed,
          ux_mode: 'popup',
          context,
        });
        gis.renderButton(element, {
          type: 'standard',
          theme: 'outline',
          size: 'large',
          text: 'continue_with',
          shape: 'pill',
          width: BUTTON_WIDTH,
        });
      },
      () => {
        if (!cancelled) setLoadFailed(true);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [clientId, context, loadGis, nonce]);

  return (
    <View style={styles.box}>
      <View ref={host} testID="google-web-button" />
      {loadFailed || !nonce ? <Text tone="muted">{t('googleWeb.unavailable')}</Text> : null}
    </View>
  );
}

const styles = StyleSheet.create({
  box: { alignItems: 'center', minHeight: 44 },
});
