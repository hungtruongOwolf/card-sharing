import { act, screen, userEvent } from '@testing-library/react-native';
import { AppState, type AppStateStatus } from 'react-native';

import { t, type MessageKey } from '../../../i18n';
import { fakeCamera } from '../../../test/onboardingHarness';
import { renderThemed } from '../../../test/render';
import type { CameraPermissionStatus } from '../../onboarding';
import {
  toCaptureOutcome,
  type CapturedPhoto,
  type CaptureOutcome,
  type CaptureSource,
} from '../captureSource';
import { CaptureScreen } from '../CaptureScreen';

const PHOTO: CapturedPhoto = {
  uri: 'file:///cache/a.jpg',
  width: 300,
  height: 400,
  origin: 'camera',
};
const LIBRARY_PHOTO: CapturedPhoto = { ...PHOTO, uri: 'file:///cache/b.jpg', origin: 'library' };

function fakeSource(pending: CapturedPhoto | null = null) {
  return {
    takePhoto: jest.fn(async (): Promise<CaptureOutcome> => ({ kind: 'photo', photo: PHOTO })),
    pickFromLibrary: jest.fn(async (): Promise<CaptureOutcome> => ({
      kind: 'photo',
      photo: LIBRARY_PHOTO,
    })),
    recoverPending: jest.fn(async () => pending),
  } satisfies CaptureSource;
}

async function renderCapture(
  status: CameraPermissionStatus,
  options: { source?: ReturnType<typeof fakeSource>; onUse?: (photo: CapturedPhoto) => void } = {},
) {
  const camera = fakeCamera(status);
  const source = options.source ?? fakeSource();
  await renderThemed(<CaptureScreen source={source} permissions={camera} onUse={options.onUse} />);
  return { camera, source };
}

const button = (key: MessageKey) => screen.getByRole('button', { name: t(key) });

describe('CaptureScreen', () => {
  it('takes a photo when the camera is allowed, then offers retake', async () => {
    const onUse = jest.fn();
    const { source } = await renderCapture('granted', { onUse });
    const user = userEvent.setup();
    await user.press(await screen.findByRole('button', { name: t('capture.take') }));
    expect(source.takePhoto).toHaveBeenCalledTimes(1);
    expect(await screen.findByLabelText(t('capture.preview.label'))).toBeTruthy();
    await user.press(button('capture.retake'));
    expect(source.takePhoto).toHaveBeenCalledTimes(2);
    await user.press(button('capture.use'));
    expect(onUse).toHaveBeenCalledWith(PHOTO);
  });

  it('explains before asking for the camera', async () => {
    const { camera, source } = await renderCapture('undetermined');
    expect(await screen.findByText(t('capture.body'))).toBeTruthy();
    expect(camera.request).not.toHaveBeenCalled();
    await userEvent.setup().press(button('capture.allow'));
    expect(camera.request).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole('button', { name: t('capture.take') })).toBeTruthy();
    expect(source.takePhoto).not.toHaveBeenCalled();
  });

  it('denied for good (AC-3.1): Open Settings, library import still works', async () => {
    const { camera, source } = await renderCapture('blocked');
    const user = userEvent.setup();
    expect(await screen.findByText(t('capture.deniedBody'))).toBeTruthy();
    expect(screen.queryByRole('button', { name: t('capture.take') })).toBeNull();
    await user.press(button('capture.openSettings'));
    expect(camera.openSettings).toHaveBeenCalledTimes(1);
    await user.press(button('capture.library'));
    expect(source.pickFromLibrary).toHaveBeenCalledTimes(1);
    expect(await screen.findByLabelText(t('capture.preview.label'))).toBeTruthy();
    expect(button('capture.chooseAnother')).toBeTruthy();
    expect(screen.queryByRole('button', { name: t('capture.use') })).toBeNull();
  });

  it('re-checks permission when the app returns from Settings', async () => {
    const listeners: ((state: AppStateStatus) => void)[] = [];
    const original = AppState.addEventListener;
    AppState.addEventListener = jest.fn((_type, listener) => {
      listeners.push(listener as (state: AppStateStatus) => void);
      return { remove: jest.fn() };
    }) as typeof AppState.addEventListener;
    try {
      const { camera } = await renderCapture('blocked');
      await screen.findByText(t('capture.deniedBody'));
      camera.status = 'granted';
      await act(async () => {
        for (const listener of listeners) listener('active');
      });
      expect(await screen.findByRole('button', { name: t('capture.take') })).toBeTruthy();
    } finally {
      AppState.addEventListener = original;
    }
  });

  it('a cancelled capture stays put; start over returns to the choices', async () => {
    const source = fakeSource();
    source.takePhoto.mockResolvedValueOnce({ kind: 'cancelled' });
    await renderCapture('granted', { source });
    const user = userEvent.setup();
    await user.press(await screen.findByRole('button', { name: t('capture.take') }));
    expect(screen.getByTestId('capture')).toBeTruthy();
    await user.press(button('capture.take'));
    await user.press(await screen.findByRole('button', { name: t('capture.startOver') }));
    expect(screen.getByTestId('capture')).toBeTruthy();
  });

  it('shows an error toast when the picker fails', async () => {
    const source = fakeSource();
    source.pickFromLibrary.mockRejectedValueOnce(new Error('no activity'));
    await renderCapture('granted', { source });
    await userEvent
      .setup()
      .press(await screen.findByRole('button', { name: t('capture.library') }));
    expect(await screen.findByText(t('capture.failed'))).toBeTruthy();
  });

  it('restores a photo taken before Android destroyed the activity', async () => {
    await renderCapture('granted', { source: fakeSource(PHOTO) });
    expect(await screen.findByTestId('capture-preview')).toBeTruthy();
  });
});

describe('toCaptureOutcome', () => {
  it('keeps the first image and treats cancel, empty and video results as cancelled', () => {
    const image = { uri: 'file:///x.jpg', width: 10, height: 20, type: 'image' };
    expect(toCaptureOutcome({ canceled: false, assets: [image] }, 'library')).toEqual({
      kind: 'photo',
      photo: { uri: 'file:///x.jpg', width: 10, height: 20, origin: 'library' },
    });
    expect(toCaptureOutcome({ canceled: true, assets: null }, 'camera')).toEqual({
      kind: 'cancelled',
    });
    expect(toCaptureOutcome({ canceled: false, assets: [] }, 'camera')).toEqual({
      kind: 'cancelled',
    });
    expect(
      toCaptureOutcome({ canceled: false, assets: [{ ...image, type: 'video' }] }, 'camera'),
    ).toEqual({ kind: 'cancelled' });
  });
});
