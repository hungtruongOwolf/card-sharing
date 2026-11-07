import { screen, userEvent, waitFor } from '@testing-library/react-native';

import { en } from '../../../i18n/messages/en';
import { t } from '../../../i18n';
import { FakeOnboardingServer } from '../../../test/fakeOnboardingServer';
import { problem } from '../../../test/fakeAuthServer';
import { fakeCamera, fakeProvider, renderOnboarding } from '../../../test/onboardingHarness';

async function fillBirthDate(
  user: ReturnType<typeof userEvent.setup>,
  m: string,
  d: string,
  y: string,
) {
  await user.type(screen.getByLabelText(t('onboarding.birthDate.month')), m);
  await user.type(screen.getByLabelText(t('onboarding.birthDate.day')), d);
  await user.type(screen.getByLabelText(t('onboarding.birthDate.year')), y);
}

const continueButton = () => screen.getByRole('button', { name: t('onboarding.continue') });

describe('onboarding flow', () => {
  it('goes sign-in → DOB → display name → handle → camera → home', async () => {
    const server = new FakeOnboardingServer();
    const provider = fakeProvider('apple');
    const user = userEvent.setup();
    await renderOnboarding(server, { providers: [provider], camera: fakeCamera('undetermined') });

    await user.press(await screen.findByRole('button', { name: t('onboarding.signIn.apple') }));
    expect(server.count('POST', '/v1/auth/apple')).toBe(1);

    await screen.findByTestId('onboarding-birth-date');
    await fillBirthDate(user, '01', '31', '2000');
    await user.press(screen.getByRole('button', { name: t('onboarding.birthDate.submit') }));

    await screen.findByTestId('onboarding-display-name');
    expect(server.birthDate).toBe('2000-01-31');
    await user.type(screen.getByLabelText(t('onboarding.displayName.label')), '  Mai ');
    await user.press(continueButton());

    await screen.findByTestId('onboarding-handle');
    expect(server.displayName).toBe('Mai');
    await user.type(screen.getByLabelText(t('onboarding.handle.label')), 'Mai.Pet');
    await user.press(continueButton());

    await screen.findByTestId('onboarding-camera');
    expect(server.handle).toBe('mai.pet');
    await user.press(screen.getByRole('button', { name: t('onboarding.camera.allow') }));
    await screen.findByTestId('home');
  });

  it('shows neutral refusal copy, clears the session and returns to start (AC-2.1)', async () => {
    const server = new FakeOnboardingServer();
    server.minAge = 16;
    const user = userEvent.setup();
    const { store } = await renderOnboarding(server, { signedIn: true });

    await screen.findByTestId('onboarding-birth-date');
    await fillBirthDate(user, '06', '01', '2011');
    await user.press(screen.getByRole('button', { name: t('onboarding.birthDate.submit') }));

    await screen.findByTestId('onboarding-refused');
    const shown = screen.getByTestId('onboarding-refused');
    // Neutral: no number (age, threshold, countdown) and no age wording anywhere on the screen.
    expect(shown).not.toHaveTextContent(/\d/);
    expect(shown).not.toHaveTextContent(/\b(age|old|young|years?|minimum|at least)\b/i);
    await waitFor(() => expect(store.current).toBeNull());

    await user.press(screen.getByRole('button', { name: t('onboarding.refused.back') }));
    await screen.findByTestId('onboarding-sign-in');
  });

  it('never offers the DOB again once submitted (AC-2.3)', async () => {
    const server = new FakeOnboardingServer();
    server.resetAccount('pending_profile');
    await renderOnboarding(server, { signedIn: true });
    await screen.findByTestId('onboarding-display-name');
    expect(screen.queryByTestId('onboarding-birth-date')).toBeNull();
  });

  it('moves on without an edit when the DOB was already submitted elsewhere', async () => {
    const server = new FakeOnboardingServer();
    const user = userEvent.setup();
    await renderOnboarding(server, { signedIn: true });
    await screen.findByTestId('onboarding-birth-date');
    // Another device submitted meanwhile: the server now answers AGE_ALREADY_SUBMITTED.
    server.resetAccount('pending_profile');
    await fillBirthDate(user, '01', '01', '1990');
    await user.press(screen.getByRole('button', { name: t('onboarding.birthDate.submit') }));
    await screen.findByTestId('onboarding-display-name');
    expect(server.birthDate).toBeNull();
  });

  it('offers a retry when /v1/me fails', async () => {
    const server = new FakeOnboardingServer();
    server.overrides.set('/v1/me', problem(503, 'TEMPORARILY_UNAVAILABLE'));
    const user = userEvent.setup();
    await renderOnboarding(server, { signedIn: true });
    await user.press(await screen.findByRole('button', { name: t('onboarding.error.retry') }));
    await screen.findByTestId('onboarding-birth-date');
  });

  it('says the server is busy (not unreachable) on 503 SERVER_BUSY', async () => {
    const server = new FakeOnboardingServer();
    server.overrides.set('/v1/me', problem(503, 'SERVER_BUSY'));
    const user = userEvent.setup();
    await renderOnboarding(server, { signedIn: true });
    expect(await screen.findByText(t('error.serverBusy'))).toBeTruthy();
    expect(screen.queryByText(t('onboarding.error.body'))).toBeNull();
    await user.press(screen.getByRole('button', { name: t('onboarding.error.retry') }));
    await screen.findByTestId('onboarding-birth-date');
  });

  it('asks for the name again when the handle claim leaves the profile step open', async () => {
    const server = new FakeOnboardingServer();
    server.resetAccount('pending_profile');
    const user = userEvent.setup();
    await renderOnboarding(server, { signedIn: true });
    await user.type(await screen.findByLabelText(t('onboarding.displayName.label')), 'Mai');
    server.overrides.set('/v1/me/profile', new Response('{}', { status: 200 })); // name lost server-side
    await user.press(continueButton());
    await user.type(await screen.findByLabelText(t('onboarding.handle.label')), 'mai');
    await user.press(continueButton());
    await screen.findByTestId('onboarding-display-name');
  });
});

describe('profile screens', () => {
  it('validates the display name and maps a profanity rejection', async () => {
    const server = new FakeOnboardingServer();
    server.resetAccount('pending_profile');
    const user = userEvent.setup();
    await renderOnboarding(server, { signedIn: true });
    const field = await screen.findByLabelText(t('onboarding.displayName.label'));

    await user.type(field, 'M');
    await user.press(continueButton());
    expect(screen.getByText(t('onboarding.displayName.tooShort', { min: 2 }))).toBeTruthy();
    expect(server.count('PUT', '/v1/me/profile')).toBe(0);

    await user.clear(field);
    await user.type(field, 'badword');
    await user.press(continueButton());
    expect(await screen.findByRole('alert')).toHaveTextContent(
      t('onboarding.displayName.rejected'),
    );
  });

  it('lowercases the handle, validates it and reports a taken one', async () => {
    const server = new FakeOnboardingServer();
    server.resetAccount('pending_profile');
    server.displayName = 'Mai';
    const user = userEvent.setup();
    await renderOnboarding(server, { signedIn: true });
    await user.type(await screen.findByLabelText(t('onboarding.displayName.label')), 'Mai');
    await user.press(continueButton());
    const field = await screen.findByLabelText(t('onboarding.handle.label'));

    await user.type(field, 'mai-pet');
    await user.press(continueButton());
    expect(screen.getByText(t('onboarding.handle.invalid', { min: 3, max: 20 }))).toBeTruthy();
    expect(server.count('PUT', '/v1/me/handle')).toBe(0);

    await user.clear(field);
    await user.type(field, 'TAKEN');
    expect(field.props.value).toBe('taken');
    await user.press(continueButton());
    expect(await screen.findByRole('alert')).toHaveTextContent(t('onboarding.handle.taken'));
  });
});

describe('catalog', () => {
  it('keeps every onboarding age-gate string free of thresholds and age wording (AC-2.4)', () => {
    const keys = Object.keys(en).filter(
      (k) => k.startsWith('onboarding.birthDate.') || k.startsWith('onboarding.refused.'),
    ) as (keyof typeof en)[];
    expect(keys.length).toBeGreaterThan(5);
    for (const key of keys) {
      expect(en[key]).not.toMatch(/\d/);
      expect(en[key]).not.toMatch(/\b(age|old|older|young|minimum|at least|must be)\b/i);
    }
  });
});
