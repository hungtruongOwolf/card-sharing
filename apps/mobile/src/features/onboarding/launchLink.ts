import { useEffect, useState } from 'react';
import { Linking, Platform } from 'react-native';

import { parseLinkFragment, type LinkFragment } from './magicLink';
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

/** Whether a URL is a sign-in link to the landing route. */
function isMagicLinkUrl(url: string | null | undefined): boolean {
  if (!url || !fragmentOf(url)) return false;
  const path = url.replace(/^[a-z][a-z0-9+.-]*:\/\/[^/]*/i, '').split(/[?#]/)[0] ?? '';
  return MAGIC_LINK_PATH.test(path);
}

/**
 * Whether the landing route may show (`LINK_ONLY_ROUTES`): the app was opened with a sign-in link and nobody has
 * signed in since. Once the phase moves past sign-in the link is spent, so a later sign-out lands on the sign-in
 * screen, not on an old link.
 */
export function useMagicLinkLanding(phase: OnboardingPhase): boolean {
  const [opened, setOpened] = useState(
    () => Platform.OS === 'web' && capturedLaunchLink() !== null,
  );
  useEffect(() => {
    if (Platform.OS === 'web') return;
    let cancelled = false;
    void Linking.getInitialURL()
      .then((url) => {
        if (!cancelled && isMagicLinkUrl(url)) setOpened(true);
      })
      .catch(() => undefined);
    const subscription = Linking.addEventListener('url', ({ url }) => {
      if (isMagicLinkUrl(url)) setOpened(true);
    });
    return () => {
      cancelled = true;
      subscription.remove();
    };
  }, []);
  // Adjusted during render (React's "reset state when a value changes" pattern), not in an effect.
  const signedInSince = phase !== 'signIn' && phase !== 'loading' && phase !== 'error';
  if (opened && signedInSince) setOpened(false);
  return opened && !signedInSince;
}
