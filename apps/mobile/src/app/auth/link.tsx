import { useRouter } from 'expo-router';
import { useEffect } from 'react';
import { Platform } from 'react-native';

import {
  launchLinkInUrl,
  MagicLinkScreen,
  useMagicLinkFragment,
  useOnboarding,
  usePendingLink,
} from '../../features/onboarding';

/**
 * Landing route of emailed links (`API_MAGIC_LINK_URL`, `/auth/link#rid=<request id>&token=<token>[&for=reauth]`,
 * ADR-0012): sign-in links before sign-in, re-auth links at home. The fragment is read once at start-up and removed from the address bar (`captureLaunchLink`); nothing in
 * it is trusted beyond its shape, and the server checks the token against this device's verifier.
 */
export default function MagicLinkRoute() {
  const router = useRouter();
  const { magicLink } = useOnboarding();
  const pendingLink = usePendingLink();
  const link = useMagicLinkFragment();
  // Expo Router keeps the URL's hash (with the token) in its state and writes it back: on the web the route first
  // replaces itself without it, and only then shows the screen, so the screen's lookup runs once (a stale request is
  // reported as expired, then forgotten).
  const hashInUrl = Platform.OS === 'web' && launchLinkInUrl();
  useEffect(() => {
    if (hashInUrl) router.replace('/auth/link');
  }, [hashInUrl, router]);
  if (hashInUrl) return null;
  return (
    <MagicLinkScreen
      link={link}
      magicLink={magicLink}
      onContinueAt={pendingLink.remember}
      onRestart={() => router.replace('/onboarding/sign-in')}
      onDone={() => router.replace('/')}
    />
  );
}
