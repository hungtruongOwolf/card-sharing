import { router } from 'expo-router';
import { act, renderRouter, screen, testRouter } from 'expo-router/testing-library';
import { useSyncExternalStore } from 'react';
import { Text } from 'react-native';

import type { OnboardingPhase } from '../phase';
import { GuardedStack, useIntendedLink } from '../GuardedStack';
import { intendedHref, PHASE_ROUTES, trackIntendedLink } from '../phaseRoutes';

/** The phase as the onboarding provider would report it, driven by the test. */
let currentPhase: OnboardingPhase = 'loading';
/** Whether the app was opened with a sign-in link (useMagicLinkLanding). */
let magicLinkLanding = false;
const listeners = new Set<() => void>();
const usePhase = () =>
  useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => currentPhase,
  );
async function setPhase(phase: OnboardingPhase) {
  await act(async () => {
    currentPhase = phase;
    listeners.forEach((listener) => listener());
  });
}

/** The root layout's phase handling (src/app/_layout.tsx) without the providers. */
function RootLayout() {
  const phase = usePhase();
  const returnTo = useIntendedLink(phase);
  if (phase === 'loading' || phase === 'error') return <Text>{`status:${phase}`}</Text>;
  return <GuardedStack phase={phase} returnTo={returnTo} magicLinkLanding={magicLinkLanding} />;
}

function screenFor(name: string) {
  return function RouteScreen() {
    return <Text>{`screen:${name}`}</Text>;
  };
}

const APP = {
  _layout: RootLayout,
  ...Object.fromEntries(PHASE_ROUTES.map(([, name]) => [name, screenFor(name)])),
};

async function open(url: string, phase: OnboardingPhase = 'loading', landing = false) {
  currentPhase = phase;
  magicLinkLanding = landing;
  return renderRouter(APP, { initialUrl: url });
}

describe('phase routes on a direct URL (R1)', () => {
  it.each([
    ['/friends', 'friends/index'],
    ['/settings/privacy', 'settings/privacy'],
    ['/card/c1', 'card/[id]'],
    ['/board/b1', 'board/[id]'],
    ['/i/tok3n', 'i/[token]'],
  ])('keeps %s for an onboarded user once loading ends (load and refresh)', async (url, route) => {
    const app = await open(url);
    expect(screen.getByText('status:loading')).toBeTruthy();
    await setPhase('home');
    expect(app.getPathname()).toBe(url);
    expect(screen.getByText(`screen:${route}`)).toBeTruthy();
  });

  it('opens the link after sign-in and every onboarding step, in order (AC-2.3 kept)', async () => {
    const app = await open('/card/c1');
    await setPhase('signIn');
    expect(app.getPathname()).toBe('/onboarding/sign-in');
    await setPhase('birthDate');
    expect(app.getPathname()).toBe('/onboarding/birth-date');
    await setPhase('displayName');
    await setPhase('handle');
    await setPhase('camera');
    expect(app.getPathname()).toBe('/onboarding/camera');
    await setPhase('home');
    expect(app.getPathname()).toBe('/card/c1');
    expect(screen.getByText('screen:card/[id]')).toBeTruthy();
    // The finished DOB step stays unreachable, by URL too.
    await act(async () => router.push('/onboarding/birth-date'));
    expect(app.getPathname()).not.toBe('/onboarding/birth-date');
    expect(screen.queryByText('screen:onboarding/birth-date')).toBeNull();
  });

  it('does not open an onboarding step from the URL before its phase', async () => {
    const app = await open('/onboarding/handle');
    await setPhase('birthDate');
    expect(app.getPathname()).toBe('/onboarding/birth-date');
    await setPhase('displayName');
    expect(app.getPathname()).toBe('/onboarding/display-name');
  });

  it('goes back through the stack', async () => {
    const app = await open('/friends', 'home');
    await testRouter.push('/settings/privacy');
    expect(app.getPathname()).toBe('/settings/privacy');
    await testRouter.back('/friends');
    expect(screen.getByText('screen:friends/index')).toBeTruthy();
  });

  it('forgets where the user was when they sign out', async () => {
    const app = await open('/settings/sessions', 'home');
    await setPhase('signIn');
    expect(app.getPathname()).toBe('/onboarding/sign-in');
    await setPhase('home');
    expect(app.getPathname()).toBe('/');
  });
});

describe('intendedHref', () => {
  it('returns signed-in screens with their query, and nothing else', () => {
    expect(intendedHref(['card', '[id]'], '/card/c1', { id: 'c1' })).toBe('/card/c1');
    expect(intendedHref(['friends'], '/friends', {})).toBe('/friends');
    expect(intendedHref(['friends', 'index'], '/friends', {})).toBe('/friends');
    expect(intendedHref(['board', '[id]'], '/board/b1', { id: 'b1', from: 'a b' })).toBe(
      '/board/b1?from=a%20b',
    );
    expect(intendedHref([], '/', {})).toBeNull();
    expect(intendedHref(['onboarding', 'birth-date'], '/onboarding/birth-date', {})).toBeNull();
    expect(
      intendedHref(['account', 'deletion-pending'], '/account/deletion-pending', {}),
    ).toBeNull();
    expect(intendedHref(['+not-found'], '/nope', {})).toBeNull();
  });
});

describe('trackIntendedLink', () => {
  it('remembers a signed-in screen before home and drops it on arrival', () => {
    let state = trackIntendedLink(null, 'loading', '/card/c1');
    state = trackIntendedLink(state, 'signIn', null);
    expect(state.intended).toBe('/card/c1');
    state = trackIntendedLink(state, 'home', null);
    expect(state.intended).toBe('/card/c1');
    state = trackIntendedLink(state, 'home', '/card/c1');
    expect(state.intended).toBeNull();
  });

  it('forgets the screen the user was on when they leave home', () => {
    const home = trackIntendedLink(null, 'home', '/settings/sessions');
    expect(trackIntendedLink(home, 'signIn', '/settings/sessions').intended).toBeNull();
  });

  it('takes the latest link that arrives before home', () => {
    let state = trackIntendedLink(null, 'signIn', '/card/c1');
    state = trackIntendedLink(state, 'signIn', null);
    state = trackIntendedLink(state, 'signIn', '/i/tok3n');
    expect(state.intended).toBe('/i/tok3n');
  });
});

describe('magic-link landing route (ADR-0012)', () => {
  it('shows only when the app was opened with a sign-in link, then the onboarding takes over', async () => {
    const app = await open('/auth/link', 'loading', true);
    await setPhase('signIn');
    expect(app.getPathname()).toBe('/auth/link');
    expect(screen.getByText('screen:auth/link')).toBeTruthy();
    await setPhase('birthDate');
    expect(app.getPathname()).toBe('/onboarding/birth-date');
  });

  it('is unavailable otherwise: a link to it lands on the sign-in screen', async () => {
    const app = await open('/auth/link');
    await setPhase('signIn');
    expect(app.getPathname()).toBe('/onboarding/sign-in');
  });
});
