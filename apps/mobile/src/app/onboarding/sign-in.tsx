import { useLocalSearchParams } from 'expo-router';

import { SignInScreen } from '../../features/onboarding';

/**
 * The sign-in screen. `?invite=<code>` (the sign-up link staff send, ADR-0012) prefills the invite code used if the
 * sign-in creates an account; the screen keeps only a plausible code and the server judges it.
 */
export default function SignInRoute() {
  const { invite } = useLocalSearchParams<{ invite?: string | string[] }>();
  return <SignInScreen inviteCode={typeof invite === 'string' ? invite : undefined} />;
}
