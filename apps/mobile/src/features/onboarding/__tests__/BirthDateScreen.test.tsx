import { screen, userEvent } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { FakeOnboardingServer } from '../../../test/fakeOnboardingServer';
import { problem } from '../../../test/fakeAuthServer';
import { renderOnboarding } from '../../../test/onboardingHarness';

const FIELDS = ['month', 'day', 'year'] as const;
const field = (part: (typeof FIELDS)[number]) =>
  screen.getByLabelText(t(`onboarding.birthDate.${part}`));
const submit = () => screen.getByRole('button', { name: t('onboarding.birthDate.submit') });

async function renderAtAgeStep(minAge = 16) {
  const server = new FakeOnboardingServer();
  server.minAge = minAge;
  const result = await renderOnboarding(server, { signedIn: true });
  await screen.findByTestId('onboarding-birth-date');
  return { server, ...result };
}

describe('BirthDateScreen (AC-2.4 checklist)', () => {
  it('1. starts with every field empty: no pre-filled date', async () => {
    await renderAtAgeStep();
    for (const part of FIELDS) expect(field(part).props.value).toBe('');
    expect(submit()).toBeDisabled();
  });

  it('2. shows no threshold, even though /v1/me carries one', async () => {
    const { server } = await renderAtAgeStep(16);
    const root = screen.getByTestId('onboarding-birth-date');
    expect(root).not.toHaveTextContent(/\d/);
    expect(root).not.toHaveTextContent(/\b(age|old|young|minimum|at least|must be)\b/i);
    for (const part of FIELDS) {
      const { accessibilityLabel, accessibilityHint, placeholder } = field(part).props as Record<
        string,
        unknown
      >;
      for (const text of [accessibilityLabel, accessibilityHint, placeholder]) {
        expect(String(text ?? '')).not.toMatch(/\d|\bage\b/i);
      }
    }
    expect(server.count('GET', '/v1/me')).toBe(1);
  });

  it('3. sends any real date unchanged; the same error for every invalid one', async () => {
    const { server } = await renderAtAgeStep();
    const user = userEvent.setup();

    await user.type(field('month'), '02');
    await user.type(field('day'), '30');
    await user.type(field('year'), '2001');
    await user.press(submit());
    expect(screen.getByRole('alert')).toHaveTextContent(t('onboarding.birthDate.invalid'));
    expect(server.count('POST', '/v1/me/age')).toBe(0);

    await user.clear(field('year'));
    await user.type(field('year'), '2030');
    await user.press(submit());
    expect(screen.getByRole('alert')).toHaveTextContent(t('onboarding.birthDate.invalid'));

    // A very recent real date is sent as is: the client never decides admission.
    await user.clear(field('day'));
    await user.type(field('day'), '01');
    await user.clear(field('year'));
    await user.type(field('year'), '2025');
    await user.press(submit());
    await screen.findByTestId('onboarding-refused');
    expect(server.auth.requests.find((r) => r.path === '/v1/me/age')?.body).toEqual({
      birth_date: '2025-02-01',
    });
  });

  it('keeps only digits and moves to the next field when one is full', async () => {
    await renderAtAgeStep();
    const user = userEvent.setup();
    await user.type(field('month'), '1a2');
    expect(field('month').props.value).toBe('12');
    await user.type(field('day'), '5');
    expect(field('day').props.value).toBe('5');
  });

  it('8. labels each field, uses the number pad and keeps the action reachable at 200 %', async () => {
    await renderAtAgeStep();
    for (const part of FIELDS) {
      expect(field(part).props.keyboardType).toBe('number-pad');
      expect(field(part).props.maxFontSizeMultiplier).toBe(2);
    }
  });

  it('shows a generic retry message when the server is unavailable', async () => {
    const { server } = await renderAtAgeStep();
    server.overrides.set('/v1/me/age', problem(503, 'TEMPORARILY_UNAVAILABLE'));
    const user = userEvent.setup();
    await user.type(field('month'), '01');
    await user.type(field('day'), '01');
    await user.type(field('year'), '1990');
    await user.press(submit());
    expect(await screen.findByRole('alert')).toHaveTextContent(t('onboarding.error.generic'));
    expect(screen.getByTestId('onboarding-birth-date')).toBeTruthy();
  });
});
