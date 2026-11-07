import { useEffect, useState } from 'react';
import { Linking, Platform } from 'react-native';

import { parseLinkFragment, type LinkFragment, type LinkPurpose } from './magicLink';
import type { OnboardingPhase } from './phase';

/** The app's magic-link landing route (`API_MAGIC_LINK_URL`, ADR-0012). */
export const MAGIC_LINK_PATH = /^\/auth\/link\/?$/;

let launchLink: LinkFragment | null = null;

interface LocationLike {
  readonly pathname: string;
  readonly search: string;
  readonly hash: string;
}

interface HistoryLike {
  readonly state: unknown;
  replaceState(data: unknown, unused: string, url?: string): void;
}

/**
 * Web: reads a magic link's `#rid=...&token=...` once, when the app starts on the landing route, and removes the
 * fragment from the address bar and the tab's history entry, so the token is not left in the URL. Runs at module
 * load of the root layout, before the router renders. A no-op on native and on any other route.
 */
export function captureLaunchLink(
  location: LocationLike | undefined = (globalThis as { location?: LocationLike }).location,
  history: HistoryLike | undefined = (globalThis as { history?: HistoryLike }).history,
): void {
  if (!location || typeof location.hash !== 'string' || !MAGIC_LINK_PATH.test(location.pathname)) {
    return;
  }
  const link = parseLinkFragment(location.hash);
  if (!link) return;
  launchLink = link;
  try {
    history?.replaceState(history.state, '', `${location.pathname}${location.search}`);
  } catch {
    // The token stays in the address bar; the link is single use anyway.
  }
}

/**
 * Web: whether the address bar still shows a magic link's token. Expo Router keeps a URL's hash as its `#` route param
 * and writes it back on every URL sync, so the landing route replaces itself without it (`router.replace`).
 */
export function launchLinkInUrl(
  location: LocationLike | undefined = (globalThis as { location?: LocationLike }).location,
): boolean {
  return !!location && MAGIC_LINK_PATH.test(location.pathname) && location.hash.includes('token=');
}

/** The magic link the app was started with on the web, if any. */
export function capturedLaunchLink(): LinkFragment | null {
  return launchLink;
}

/** Test hook. */
export function resetLaunchLinkForTests(link: LinkFragment | null = null): void {
  launchLink = link;
}

function fragmentOf(url: string | null | undefined): LinkFragment | null {
  if (!url) return null;
  const at = url.indexOf('#');
  return at < 0 ? null : parseLinkFragment(url.slice(at));
}

/**
 * The magic link the landing screen was opened with: on the web the captured launch link; on native the URL that
 * opened the app (or the latest one while it runs). `undefined` while it is being read, null when there is none.
 */
export function useMagicLinkFragment(): LinkFragment | null | undefined {
  const [link, setLink] = useState<LinkFragment | null | undefined>(() =>
    Platform.OS === 'web' ? capturedLaunchLink() : undefined,
  );
  useEffect(() => {
    if (Platform.OS === 'web') return;
    let cancelled = false;
    void Linking.getInitialURL()
      .then((url) => {
        if (!cancelled) setLink((current) => current ?? fragmentOf(url));
      })
      .catch(() => {
        if (!cancelled) setLink((current) => current ?? null);
      });
    const subscription = Linking.addEventListener('url', ({ url }) => {
      const next = fragmentOf(url);
      if (next) setLink(next);
    });
    return () => {
      cancelled = true;
      subscription.remove();
    };
  }, []);
  return link;
}

/** The purpose of a URL that is a link to the landing route, or null. */
function landingPurpose(url: string | null | undefined): LinkPurpose | null {
  const link = fragmentOf(url);
  if (!url || !link) return null;
  const path = url.replace(/^[a-z][a-z0-9+.-]*:\/\/[^/]*/i, '').split(/[?#]/)[0] ?? '';
  return MAGIC_LINK_PATH.test(path) ? link.purpose : null;
}

/**
 * Which emailed link the landing route may show (`LINK_ONLY_ROUTES`), if any: the app was opened with a sign-in link
 * and nobody has signed in since, or with a re-auth link while signed in. A sign-in link is spent once the phase moves
 * past sign-in, so a later sign-out lands on the sign-in screen, not on an old link.
 */
export function useMagicLinkLanding(phase: OnboardingPhase): LinkPurpose | null {
  const [opened, setOpened] = useState<LinkPurpose | null>(() =>
    Platform.OS === 'web' ? (capturedLaunchLink()?.purpose ?? null) : null,
  );
  useEffect(() => {
    if (Platform.OS === 'web') return;
    let cancelled = false;
    void Linking.getInitialURL()
      .then((url) => {
        const purpose = landingPurpose(url);
        if (!cancelled && purpose) setOpened(purpose);
      })
      .catch(() => undefined);
    const subscription = Linking.addEventListener('url', ({ url }) => {
      const purpose = landingPurpose(url);
      if (purpose) setOpened(purpose);
    });
    return () => {
      cancelled = true;
      subscription.remove();
    };
  }, []);
  // Adjusted during render (React's "reset state when a value changes" pattern), not in an effect.
  const signedInSince = phase !== 'signIn' && phase !== 'loading' && phase !== 'error';
  const spent = opened === 'sign_in' && signedInSince;
  if (spent) setOpened(null);
  return spent ? null : opened;
}
