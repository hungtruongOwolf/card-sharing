import { useRouter } from 'expo-router';

import { HoloLabScreen } from '../features/holo-card';

/** T-52 holo spike lab; reachable from the home screen. */
export default function HoloLabRoute() {
  const router = useRouter();
  return <HoloLabScreen onClose={() => router.back()} />;
}
