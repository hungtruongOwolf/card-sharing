import { screen, userEvent, waitFor } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { FakeOnboardingServer } from '../../../test/fakeOnboardingServer';
import { GRACE_MS, FakePrivacyServer } from '../../../test/fakePrivacyServer';
import { fakeCamera, renderOnboarding } from '../../../test/onboardingHarness';
import { formatDateTime } from '../privacyMessages';

function setup() {
  const onboarding = new FakeOnboardingServer();
  onboarding.resetAccount('deletion_pending');
  const privacy = new FakePrivacyServer(onboarding.auth);
  const now = onboarding.auth.nowMs;
  privacy.deletion = { requestedAt: now, graceUntil: now + GRACE_MS };
  privacy.onDeletionCancelled = () => {
    onboarding.status = 'active';
  };
  return { onboarding, privacy, graceUntil: now + GRACE_MS };
}

describe('deletion-pending account (AC-29.2)', () => {
  it('shows only the grace banner, then cancels and opens the app', async () => {
    const { onboarding, privacy, graceUntil } = setup();
    const user = userEvent.setup();
    await renderOnboarding(onboarding, { signedIn: true, camera: fakeCamera('granted') });

    expect(await screen.findByTestId('deletion-pending')).toBeTruthy();
    expect(
      await screen.findByText(t('deletionPending.banner', { date: formatDateTime(graceUntil) })),
    ).toBeTruthy();
    expect(screen.queryByTestId('home')).toBeNull();

    await user.press(screen.getByRole('button', { name: t('deletionPending.cancel') }));
    await waitFor(() => expect(privacy.deletion).toBeNull());
    // The account is active again: the app opens (camera already granted, so straight to home).
    expect(await screen.findByTestId('home')).toBeTruthy();
  });

  it('signs out without cancelling', async () => {
    const { onboarding, privacy } = setup();
    const user = userEvent.setup();
    const { authClient } = await renderOnboarding(onboarding, { signedIn: true });
    await screen.findByTestId('deletion-pending');
    await user.press(screen.getByRole('button', { name: t('deletionPending.signOut') }));
    await waitFor(() => expect(authClient.getState().status).toBe('signedOut'));
    expect(privacy.deletion).not.toBeNull();
    expect(await screen.findByTestId('onboarding-sign-in')).toBeTruthy();
  });

  it('a grace period that already ended re-reads the account', async () => {
    const { onboarding, privacy } = setup();
    const user = userEvent.setup();
    await renderOnboarding(onboarding, { signedIn: true });
    await screen.findByTestId('deletion-pending');
    onboarding.auth.advance(GRACE_MS);
    await user.press(screen.getByRole('button', { name: t('deletionPending.cancel') }));
    expect(await screen.findByText(t('deletionPending.gone'))).toBeTruthy();
    expect(privacy.deletion).not.toBeNull();
  });
});
