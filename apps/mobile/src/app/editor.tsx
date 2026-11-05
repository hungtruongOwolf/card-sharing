import Constants from 'expo-constants';
import { Redirect, useLocalSearchParams, useRouter } from 'expo-router';
import { useMemo } from 'react';

import {
  CardEditorScreen,
  createSyntheticMaskSource,
  noMaskSource,
  parseEditorPhoto,
} from '../features/editor';
import { useRemoteConfig } from '../features/remote-config';

/**
 * Card editor (T-64). The photo arrives as route params from capture. Until the real segmenters land
 * (T-54/T-55), development builds use a synthetic subject mask and release builds make whole-photo cards.
 */
export default function EditorRoute() {
  const router = useRouter();
  const params = useLocalSearchParams();
  const photo = useMemo(() => parseEditorPhoto(params), [params]);
  const maskSource = useMemo(() => (__DEV__ ? createSyntheticMaskSource() : noMaskSource), []);
  const remoteConfig = useRemoteConfig();
  if (!photo) return <Redirect href="/capture" />;
  return (
    <CardEditorScreen
      photo={photo}
      maskSource={maskSource}
      appVersion={Constants.expoConfig?.version ?? '0.0.0'}
      remoteConfig={remoteConfig?.registry}
      onBack={() => (router.canGoBack() ? router.back() : router.replace('/capture'))}
    />
  );
}
