import { useRouter } from 'expo-router';

import {
  MagicLinkScreen,
  useMagicLinkFragment,
  useOnboarding,
  usePendingLink,
} from '../../features/onboarding';

/**
 * Landing route of emailed sign-in links (`API_MAGIC_LINK_URL`, `/auth/link#rid=<request id>&token=<token>`,
 * ADR-0012). The fragment is read once at start-up and removed from the address bar (`captureLaunchLink`); nothing in
 * it is trusted beyond its shape, and the server checks the token against this device's verifier.
 */
export default function MagicLinkRoute() {
  const router = useRouter();
  const { magicLink } = useOnboarding();
  const pendingLink = usePendingLink();
  const link = useMagicLinkFragment();
  return (
    <MagicLinkScreen
      link={link}
      magicLink={magicLink}
      onContinueAt={pendingLink.remember}
      onRestart={() => router.replace('/onboarding/sign-in')}
    />
  );
}
