import { Platform } from 'react-native';

import { base64Url, hex, asciiBytes, platformRandom, sha256 } from './pkce';

/**
 * Google Identity Services on the web (ADR-0012 decision 8): the "Sign in with Google" button and its ID token,
 * for sign-in and for re-auth. Behind `EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID` (an OAuth client id is public, not a
 * secret): unset, or not on the web, the button is not shown. The server accepts the token only when the same id is
 * in `API_GOOGLE_CLIENT_IDS` and the token's `nonce` claim is the SHA-256 of the raw nonce sent with it.
 *
 * Google allows plain-http JavaScript origins only for `http://localhost`: on `http://<pc-lan-ip>:8081` the button
 * cannot work, so testers on the home network use email links until there is an https origin.
 *
 * API used (developers.google.com/identity/gsi/web/reference/js-reference): `google.accounts.id.initialize({client_id,
 * callback, nonce, ux_mode, context})` and `google.accounts.id.renderButton(parent, {type, theme, size, text, shape,
 * width})`; the callback receives `{credential}` (the ID token).
 */

export const GIS_SCRIPT_URL = 'https://accounts.google.com/gsi/client';

/** The part of `google.accounts.id` the app uses. */
export interface GisApi {
  initialize(config: {
    client_id: string;
    callback: (response: { credential?: string }) => void;
    nonce: string;
    ux_mode: 'popup';
    context: 'signin' | 'use';
  }): void;
  renderButton(
    parent: HTMLElement,
    options: {
      type: 'standard';
      theme: 'outline';
      size: 'large';
      text: 'continue_with';
      shape: 'pill';
      width: number;
    },
  ): void;
}

const CLIENT_ID = /^[0-9]+-[a-z0-9]+\.apps\.googleusercontent\.com$/;

/** A plausible OAuth client id, or null. */
export function parseGoogleWebClientId(raw: string | undefined): string | null {
  const value = raw?.trim() ?? '';
  return CLIENT_ID.test(value) ? value : null;
}

/** The web OAuth client id, or null when unset or not on the web (the button is hidden). */
export function googleWebClientId(): string | null {
  if (Platform.OS !== 'web') return null;
  // Expo inlines EXPO_PUBLIC_* only for the literal expression.
  return parseGoogleWebClientId(process.env.EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID);
}

let loading: Promise<GisApi> | null = null;

interface GisWindow {
  google?: { accounts?: { id?: GisApi } };
}

/** Loads Google's script once and resolves with `google.accounts.id`. */
export function loadGoogleIdentity(): Promise<GisApi> {
  if (loading) return loading;
  loading = new Promise<GisApi>((resolve, reject) => {
    const win = globalThis as unknown as GisWindow & { document?: Document };
    const ready = () => win.google?.accounts?.id;
    const existing = ready();
    if (existing) {
      resolve(existing);
      return;
    }
    const doc = win.document;
    if (!doc) {
      reject(new Error('no document'));
      return;
    }
    const script = doc.createElement('script');
    script.src = GIS_SCRIPT_URL;
    script.async = true;
    script.onload = () => {
      const api = ready();
      if (api) resolve(api);
      else reject(new Error('Google Identity Services did not load'));
    };
    script.onerror = () => reject(new Error('Google Identity Services did not load'));
    doc.head.appendChild(script);
  }).catch((error: unknown) => {
    loading = null; // let a later screen try again
    throw error;
  });
  return loading;
}

/** A raw nonce for one button (43 visible ASCII characters) and what Google gets: lowercase hex SHA-256 of it. */
export function googleNonce(random = platformRandom()): { raw: string; hashed: string } | null {
  if (!random) return null;
  const raw = base64Url(random(new Uint8Array(32)));
  return { raw, hashed: hex(sha256(asciiBytes(raw))) };
}
