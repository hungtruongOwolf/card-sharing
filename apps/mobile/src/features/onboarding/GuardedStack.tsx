import { Stack, useGlobalSearchParams, usePathname, useSegments, type Href } from 'expo-router';
import { useState } from 'react';
import type { ViewStyle } from 'react-native';

import type { OnboardingPhase } from './phase';
import { intendedHref, PHASE_ROUTES, trackIntendedLink, type RoutedPhase } from './phaseRoutes';

/**
 * The stack follows the onboarding phase: when the phase changes, the guard of the current screen turns false and
 * Expo Router moves to the only screens left available and drops the old one from history, so there is no "back"
 * into a finished step (the DOB screen in particular, AC-2.3). The server enforces the same order; these guards
 * only keep the UI consistent with it.
 *
 * `returnTo` (from {@link useIntendedLink}) is where a finished onboarding step goes instead of the home screen.
 */
export function GuardedStack({
  phase,
  returnTo,
  contentStyle,
}: {
  phase: RoutedPhase;
  returnTo?: Href;
  contentStyle?: ViewStyle;
}) {
  return (
    <Stack screenOptions={{ headerShown: false, gestureEnabled: false, contentStyle }}>
      {PHASE_ROUTES.map(([routePhase, name]) => (
        <Stack.Protected
          key={name}
          guard={phase === routePhase}
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
  const href = intendedHref(useSegments(), usePathname(), useGlobalSearchParams());
  const [tracked, setTracked] = useState(() => trackIntendedLink(null, phase, href));
  // Adjusted during render (React's "reset state when a value changes" pattern), not in an effect.
  if (tracked.phase !== phase || tracked.href !== href) {
    setTracked(trackIntendedLink(tracked, phase, href));
  }
  return phase === 'home' && tracked.intended !== null ? (tracked.intended as Href) : undefined;
}
