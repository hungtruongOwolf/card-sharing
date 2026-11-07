import type { OnboardingPhase } from './phase';

/** Phases that own routes; `loading` and `error` show a status screen over no route. */
export type RoutedPhase = Exclude<OnboardingPhase, 'loading' | 'error'>;

/**
 * Routes of each phase. Only the current phase's routes are reachable (phase.ts); the first one listed is where
 * the phase lands. Guarded with `Stack.Protected` in `GuardedStack`.
 */
export const PHASE_ROUTES: readonly (readonly [RoutedPhase, string])[] = [
  ['home', 'index'],
  ['home', 'holo-lab'],
  ['home', 'capture'],
  ['home', 'editor'],
  ['home', 'drafts'],
  ['home', 'send'],
  ['home', 'feed'],
  ['home', 'activity'],
  ['home', 'card/[id]'],
  ['home', 'boards'],
  ['home', 'board/[id]'],
  ['home', 'board-lab'],
  ['home', 'outbox'],
  ['home', 'cut-lab'],
  ['home', 'effects-lab'],
  ['home', 'settings/sessions'],
  ['home', 'settings/privacy'],
  ['home', 'settings/feedback'],
  ['home', 'settings/notifications'],
  ['home', 'settings/delete-account'],
  ['home', 'settings/data-export'],
  ['home', 'friends/index'],
  ['home', 'friends/requests'],
  ['home', 'friends/invite'],
  ['home', 'friends/find'],
  ['home', 'i/[token]'],
  ['signIn', 'onboarding/sign-in'],
  ['refused', 'onboarding/refused'],
  ['birthDate', 'onboarding/birth-date'],
  ['displayName', 'onboarding/display-name'],
  ['handle', 'onboarding/handle'],
  ['camera', 'onboarding/camera'],
  ['deletionPending', 'account/deletion-pending'],
];

/** `friends/index` and `friends` name the same screen; `index` alone is the home screen (empty name). */
const normalizeRoute = (name: string) => name.replace(/(^|\/)index$/, '');

/** Home screens worth returning to after sign-in or onboarding; the home screen itself needs no return. */
const RETURNABLE_ROUTES: ReadonlySet<string> = new Set(
  PHASE_ROUTES.filter(([phase]) => phase === 'home')
    .map(([, name]) => normalizeRoute(name))
    .filter((name) => name !== ''),
);

const DYNAMIC_SEGMENT = /^\[(?:\.\.\.)?([^\]]+)\]$/;

/**
 * The link to return to once the account reaches home, or null when the current route is not a signed-in screen
 * (an onboarding step, the home screen, an unknown path). Only routes of the app's own `home` phase qualify, so
 * the remembered link can never point outside the app or at an onboarding step. `params` are the router's global
 * params: the dynamic segments are already in `pathname`, the rest is the query string.
 */
export function intendedHref(
  segments: readonly string[],
  pathname: string,
  params: Readonly<Record<string, string | string[] | undefined>>,
): string | null {
  if (!RETURNABLE_ROUTES.has(normalizeRoute(segments.join('/')))) return null;
  const dynamic = new Set(segments.flatMap((segment) => DYNAMIC_SEGMENT.exec(segment)?.[1] ?? []));
  const query = Object.entries(params)
    .filter(([name]) => !dynamic.has(name))
    .flatMap(([name, value]) =>
      (value === undefined ? [] : Array.isArray(value) ? value : [value]).map(
        (item) => `${encodeURIComponent(name)}=${encodeURIComponent(item)}`,
      ),
    );
  return query.length > 0 ? `${pathname}?${query.join('&')}` : pathname;
}

/** What {@link trackIntendedLink} remembers between renders. */
export interface IntendedLinkState {
  readonly phase: OnboardingPhase;
  /** {@link intendedHref} of the current route. */
  readonly href: string | null;
  /** The link to open once the account is home; null when there is none. */
  readonly intended: string | null;
}

/**
 * Follows the phase and the current route. Before home, a signed-in screen the app shows (the URL it was opened at,
 * or a link that arrived later) becomes the intended link; at home it is kept until the app arrives there. Leaving
 * home (sign-out, deletion) forgets it: where the user was is not a link they opened.
 */
export function trackIntendedLink(
  previous: IntendedLinkState | null,
  phase: OnboardingPhase,
  href: string | null,
): IntendedLinkState {
  const kept = previous?.intended ?? null;
  let intended: string | null;
  if (phase === 'home') intended = href !== null && href === kept ? null : kept;
  else if (previous?.phase === 'home') intended = null;
  else intended = href ?? kept;
  return { phase, href, intended };
}
