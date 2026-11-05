import { useCallback, useEffect, useState } from 'react';
import { AppState, Image, StyleSheet } from 'react-native';

import { radius } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text, useToast } from '../../ui';
import type { CameraPermissions, CameraPermissionStatus } from '../onboarding';
import type { CapturedPhoto, CaptureOutcome, CaptureSource } from './captureSource';

export interface CaptureScreenProps {
  source: CaptureSource;
  permissions: CameraPermissions;
  /** Next step for the chosen photo (cutout, T-54+). Without it the screen only previews. */
  onUse?: (photo: CapturedPhoto) => void;
}

const PREVIEW_MAX_HEIGHT = 420;

/**
 * Capture or import a photo (T-53, US-7). Camera permission states follow AC-3.1: explain before asking,
 * "Open Settings" once blocked (re-checked on return to the foreground), and library import in every state.
 */
export function CaptureScreen({ source, permissions, onUse }: CaptureScreenProps) {
  const toast = useToast();
  const [status, setStatus] = useState<CameraPermissionStatus | null>(null);
  const [photo, setPhoto] = useState<CapturedPhoto | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const refresh = () => {
      void permissions
        .get()
        .catch(() => 'undetermined' as const)
        .then((next) => {
          if (!cancelled) setStatus(next);
        });
    };
    refresh();
    void source
      .recoverPending()
      .catch(() => null)
      .then((pending) => {
        if (!cancelled && pending) setPhoto(pending);
      });
    const subscription = AppState.addEventListener('change', (state) => {
      if (state === 'active') refresh();
    });
    return () => {
      cancelled = true;
      subscription.remove();
    };
  }, [permissions, source]);

  const run = useCallback(
    async (action: () => Promise<CaptureOutcome | void>) => {
      setBusy(true);
      try {
        const outcome = await action();
        if (outcome?.kind === 'photo') setPhoto(outcome.photo);
      } catch {
        toast.show({ message: t('capture.failed'), variant: 'error' });
      } finally {
        setBusy(false);
      }
    },
    [toast],
  );

  const takePhoto = () => void run(() => source.takePhoto());
  const pickFromLibrary = () => void run(() => source.pickFromLibrary());
  const libraryButton = (
    <Button
      block
      variant="secondary"
      label={t('capture.library')}
      disabled={busy}
      onPress={pickFromLibrary}
    />
  );

  if (photo) {
    const aspect = photo.width > 0 && photo.height > 0 ? photo.width / photo.height : 1;
    return (
      <Screen title={t('capture.preview.title')} testID="capture-preview">
        <Image
          source={{ uri: photo.uri }}
          style={[styles.preview, { aspectRatio: aspect }]}
          resizeMode="contain"
          accessibilityLabel={t('capture.preview.label')}
          accessibilityIgnoresInvertColors
        />
        {onUse ? <Button block label={t('capture.use')} onPress={() => onUse(photo)} /> : null}
        <Button
          block
          variant={onUse ? 'secondary' : 'primary'}
          label={t(photo.origin === 'camera' ? 'capture.retake' : 'capture.chooseAnother')}
          disabled={busy}
          onPress={photo.origin === 'camera' ? takePhoto : pickFromLibrary}
        />
        <Button
          block
          variant="ghost"
          label={t('capture.startOver')}
          onPress={() => setPhoto(null)}
        />
      </Screen>
    );
  }

  if (status === null) return <Screen testID="capture-loading">{null}</Screen>;

  const blocked = status === 'blocked';
  return (
    <Screen title={t(blocked ? 'capture.deniedTitle' : 'capture.title')} testID="capture">
      {status === 'granted' ? (
        <Button block label={t('capture.take')} disabled={busy} onPress={takePhoto} />
      ) : (
        <>
          <Text tone="muted">{t(blocked ? 'capture.deniedBody' : 'capture.body')}</Text>
          {blocked ? (
            <Button
              block
              label={t('capture.openSettings')}
              disabled={busy}
              onPress={() => void run(() => permissions.openSettings())}
            />
          ) : (
            <Button
              block
              label={t('capture.allow')}
              disabled={busy}
              onPress={() =>
                void run(async () => {
                  setStatus(await permissions.request());
                })
              }
            />
          )}
        </>
      )}
      {libraryButton}
    </Screen>
  );
}

const styles = StyleSheet.create({
  preview: { width: '100%', maxHeight: PREVIEW_MAX_HEIGHT, borderRadius: radius.md },
});
