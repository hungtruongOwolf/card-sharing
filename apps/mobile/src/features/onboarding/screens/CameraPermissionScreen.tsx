import { useCallback, useEffect, useState } from 'react';
import { AppState } from 'react-native';

import { t } from '../../../i18n';
import { Button, Screen, Text } from '../../../ui';
import type { CameraPermissionStatus } from '../cameraPermissions';
import { useOnboarding } from '../OnboardingProvider';
import { LoadingScreen } from './StatusScreens';

/**
 * Camera explainer (US-3, AC-3.1). Asks only after explaining why; once denied for good it offers
 * "Open Settings" and re-checks when the app returns to the foreground. Library import stays available in
 * every state, and "Not now" never blocks the user.
 */
export function CameraPermissionScreen() {
  const { camera, finishCamera } = useOnboarding();
  const [status, setStatus] = useState<CameraPermissionStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const readStatus = useCallback(
    (): Promise<CameraPermissionStatus> => camera.get().catch(() => 'undetermined' as const),
    [camera],
  );

  useEffect(() => {
    let cancelled = false;
    const refresh = () => {
      void readStatus().then((next) => {
        if (!cancelled) setStatus(next);
      });
    };
    refresh();
    const subscription = AppState.addEventListener('change', (state) => {
      if (state === 'active') refresh();
    });
    return () => {
      cancelled = true;
      subscription.remove();
    };
  }, [readStatus]);

  useEffect(() => {
    if (status === 'granted') finishCamera();
  }, [status, finishCamera]);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    try {
      await action();
    } catch {
      // A failed OS call leaves the screen as it was; every option stays available.
    } finally {
      setBusy(false);
    }
  }

  if (status === null || status === 'granted') return <LoadingScreen />;

  const blocked = status === 'blocked';
  return (
    <Screen
      title={t(blocked ? 'onboarding.camera.deniedTitle' : 'onboarding.camera.title')}
      testID="onboarding-camera"
    >
      <Text tone="muted">
        {t(blocked ? 'onboarding.camera.deniedBody' : 'onboarding.camera.body')}
      </Text>
      {blocked ? (
        <Button
          block
          label={t('onboarding.camera.openSettings')}
          disabled={busy}
          onPress={() => void run(() => camera.openSettings())}
        />
      ) : (
        <Button
          block
          label={t('onboarding.camera.allow')}
          disabled={busy}
          onPress={() =>
            void run(async () => {
              setStatus(await camera.request());
            })
          }
        />
      )}
      <Button
        block
        variant="secondary"
        label={t('onboarding.camera.library')}
        disabled={busy}
        onPress={() =>
          void run(async () => {
            if (await camera.pickFromLibrary()) finishCamera();
          })
        }
      />
      <Button block variant="ghost" label={t('onboarding.camera.notNow')} onPress={finishCamera} />
    </Screen>
  );
}
