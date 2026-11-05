import { act, screen, userEvent } from '@testing-library/react-native';
import { AppState, type AppStateStatus } from 'react-native';

import { t } from '../../../i18n';
import { FakeOnboardingServer } from '../../../test/fakeOnboardingServer';
import { fakeCamera, fakeProvider, renderOnboarding } from '../../../test/onboardingHarness';
import type { CameraPermissionStatus } from '../cameraPermissions';

async function renderAtCamera(status: CameraPermissionStatus) {
  const server = new FakeOnboardingServer();
  server.resetAccount('active');
  const camera = fakeCamera(status);
  await renderOnboarding(server, { signedIn: true, camera });
  return camera;
}

const button = (key: Parameters<typeof t>[0]) => screen.getByRole('button', { name: t(key) });

describe('CameraPermissionScreen (AC-3.1)', () => {
  it('explains before asking, then continues when granted', async () => {
    const camera = await renderAtCamera('undetermined');
    await screen.findByTestId('onboarding-camera');
    expect(screen.getByText(t('onboarding.camera.body'))).toBeTruthy();
    expect(camera.request).not.toHaveBeenCalled();
    await userEvent.setup().press(button('onboarding.camera.allow'));
    expect(camera.request).toHaveBeenCalledTimes(1);
    await screen.findByTestId('home');
  });

  it('denied: shows the explainer with Open Settings and keeps library import', async () => {
    const camera = await renderAtCamera('undetermined');
    camera.answer = 'blocked';
    const user = userEvent.setup();
    await user.press(await screen.findByRole('button', { name: t('onboarding.camera.allow') }));

    expect(await screen.findByText(t('onboarding.camera.deniedBody'))).toBeTruthy();
    expect(screen.queryByRole('button', { name: t('onboarding.camera.allow') })).toBeNull();
    await user.press(button('onboarding.camera.openSettings'));
    expect(camera.openSettings).toHaveBeenCalledTimes(1);

    expect(button('onboarding.camera.library')).toBeEnabled();
    await user.press(button('onboarding.camera.library'));
    expect(camera.pickFromLibrary).toHaveBeenCalledTimes(1);
    await screen.findByTestId('home');
  });

  it('re-checks when the app returns from Settings', async () => {
    const listeners: ((state: AppStateStatus) => void)[] = [];
    // The react-native jest preset already mocks addEventListener; spyOn + mockRestore would strip that
    // mock's implementation for later tests, so swap the function and put the original back.
    const original = AppState.addEventListener;
    AppState.addEventListener = jest.fn((_type, listener) => {
      listeners.push(listener as (state: AppStateStatus) => void);
      return { remove: jest.fn() };
    }) as typeof AppState.addEventListener;
    try {
      const camera = await renderAtCamera('blocked');
      await screen.findByText(t('onboarding.camera.deniedBody'));
      camera.status = 'granted';
      await act(async () => {
        for (const listener of listeners) listener('active');
      });
      await screen.findByTestId('home');
    } finally {
      AppState.addEventListener = original;
    }
  });

  it('a cancelled library pick stays on the screen; "Not now" continues', async () => {
    const camera = await renderAtCamera('blocked');
    camera.picks = false;
    const user = userEvent.setup();
    await user.press(await screen.findByRole('button', { name: t('onboarding.camera.library') }));
    expect(screen.getByTestId('onboarding-camera')).toBeTruthy();
    await user.press(button('onboarding.camera.notNow'));
    await screen.findByTestId('home');
  });

  it('skips straight on when access is already granted', async () => {
    await renderAtCamera('granted');
    await screen.findByTestId('home');
  });
});

describe('SignInScreen', () => {
  it('passes the provider credential to the sign-in seam', async () => {
    const server = new FakeOnboardingServer();
    const google = fakeProvider('google', {
      idToken: 'valid-id-token',
      nonce: 'n0nce-0123456789abcdef',
    });
    await renderOnboarding(server, { providers: [fakeProvider('apple', 'unavailable'), google] });
    await userEvent
      .setup()
      .press(await screen.findByRole('button', { name: t('onboarding.signIn.google') }));
    await screen.findByTestId('onboarding-birth-date');
    const request = server.auth.requests.find((r) => r.path === '/v1/auth/google');
    expect(request?.body).toEqual({ id_token: 'valid-id-token', nonce: 'n0nce-0123456789abcdef' });
  });

  it('keeps unavailable providers visible and explains them', async () => {
    const server = new FakeOnboardingServer();
    const apple = fakeProvider('apple', 'unavailable');
    await renderOnboarding(server, { providers: [apple] });
    await userEvent
      .setup()
      .press(await screen.findByRole('button', { name: t('onboarding.signIn.apple') }));
    expect(screen.getByRole('alert')).toHaveTextContent(t('onboarding.signIn.unavailable'));
    expect(apple.getCredential).not.toHaveBeenCalled();
  });

  it('stays put silently when the user cancels the sheet', async () => {
    const server = new FakeOnboardingServer();
    const cancelled = fakeProvider('apple', null);
    const user = userEvent.setup();
    await renderOnboarding(server, { providers: [cancelled] });
    await user.press(await screen.findByRole('button', { name: t('onboarding.signIn.apple') }));
    expect(screen.queryByRole('alert')).toBeNull();
    expect(server.count('POST', '/v1/auth/apple')).toBe(0);
  });

  it('reports a token the server rejects', async () => {
    const server = new FakeOnboardingServer();
    const bad = fakeProvider('apple', { idToken: 'forged', nonce: 'n0nce-0123456789abcdef' });
    await renderOnboarding(server, { providers: [bad] });
    await userEvent
      .setup()
      .press(await screen.findByRole('button', { name: t('onboarding.signIn.apple') }));
    expect(await screen.findByRole('alert')).toHaveTextContent(t('onboarding.signIn.failed'));
    expect(screen.getByTestId('onboarding-sign-in')).toBeTruthy();
  });
});
