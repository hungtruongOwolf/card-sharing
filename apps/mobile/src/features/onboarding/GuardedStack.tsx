import { Stack, useGlobalSearchParams, usePathname, useSegments, type Href } from 'expo-router';
import { createContext, useCallback, useContext, useMemo, useState } from 'react';
import type { ViewStyle } from 'react-native';

import type { OnboardingPhase } from './phase';
import {
  intendedHref,
  isReturnableHref,
  LINK_ONLY_ROUTES,
  PHASE_ROUTES,
  trackIntendedLink,
  type RoutedPhase,
} from './phaseRoutes';

/**
 * The stack follows the onboarding phase: when the phase changes, the guard of the current screen turns false and
 * Expo Router moves to the only screens left available and drops the old one from history, so there is no "back"
 * into a finished step (the DOB screen in particular, AC-2.3). The server enforces the same order; these guards
 * only keep the UI consistent with it.
 *
 * `returnTo` (from {@link useIntendedLink}) is where a finished onboarding step goes instead of the home screen.
 * `magicLinkLanding` (from `useMagicLinkLanding`) opens the routes in `LINK_ONLY_ROUTES`.
 */
export function GuardedStack({
  phase,
  returnTo,
  magicLinkLanding = false,
  contentStyle,
}: {
  phase: RoutedPhase;
  returnTo?: Href;
  magicLinkLanding?: boolean;
  contentStyle?: ViewStyle;
}) {
  return (
    <Stack screenOptions={{ headerShown: false, gestureEnabled: false, contentStyle }}>
      {PHASE_ROUTES.map(([routePhase, name]) => (
        <Stack.Protected
          key={name}
          guard={phase === routePhase && (magicLinkLanding || !LINK_ONLY_ROUTES.has(name))}
          redirectTo={routePhase === 'home' ? undefined : returnTo}
        >
          <Stack.Screen name={name} />
        </Stack.Protected>
      ))}
    </Stack>
  );
}

/**
 * The link to open once the account reaches home (R1). A signed-out or not yet onboarded user who opens
 * `/card/<id>` or `/i/<token>` is guarded to the current onboarding step, which replaces the URL; the link is
 * remembered here and returned (for {@link GuardedStack}'s `returnTo`) once the phase is `home`, until the app
 * arrives there. Must run in the root layout on every phase, including `loading`, so the URL is read before any
 * guard redirects. Leaving home (sign-out, deletion) forgets where the user was: that is not a link they opened.
 */
export function useIntendedLink(phase: OnboardingPhase): Href | undefined {
  return useIntendedLinkState(phase).returnTo;
}

/** The remembered link, for screens before home ({@link IntendedLinkProvider}). */
export interface PendingLink {
  /** The link to open once the account is home, or null. */
  readonly href: string | null;
  /**
   * Remembers `href` (when nothing is remembered yet) so the app opens it once home. Used by the magic-link landing
   * screen: the link opens in a new tab, which did not see the link the user started from (ADR-0012). Only the app's
   * own signed-in routes are accepted ({@link isReturnableHref}).
   */
  remember(href: string): void;
}

const PendingLinkContext = createContext<PendingLink>({ href: null, remember: () => undefined });

/** Provides {@link usePendingLink}; the root layout wraps the stack with the value of {@link useIntendedLinkState}. */
export const IntendedLinkProvider = PendingLinkContext.Provider;

export function usePendingLink(): PendingLink {
  return useContext(PendingLinkContext);
}

/** {@link useIntendedLink} plus the {@link PendingLink} for the screens inside the stack. */
export function useIntendedLinkState(phase: OnboardingPhase): {
  returnTo: Href | undefined;
  pending: PendingLink;
} {
  const href = intendedHref(useSegments(), usePathname(), useGlobalSearchParams());
  const [tracked, setTracked] = useState(() => trackIntendedLink(null, phase, href));
  // Adjusted during render (React's "reset state when a value changes" pattern), not in an effect.
  if (tracked.phase !== phase || tracked.href !== href) {
    setTracked(trackIntendedLink(tracked, phase, href));
  }
  const remember = useCallback((link: string) => {
    if (!isReturnableHref(link)) return;
    setTracked((current) => (current.intended === null ? { ...current, intended: link } : current));
  }, []);
  const pending = useMemo(
    () => ({ href: tracked.intended, remember }),
    [tracked.intended, remember],
  );
  return {
    returnTo:
      phase === 'home' && tracked.intended !== null ? (tracked.intended as Href) : undefined,
    pending,
  };
}
