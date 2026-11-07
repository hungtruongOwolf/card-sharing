import { expect, test } from '../support/app';

test('a demo user signs in and lands on the home feed', async ({ app }) => {
  const alice = await app.signIn('alice');
  const { page } = alice;
  await expect(page.getByTestId('feed')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Make a card', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'More', exact: true })).toBeVisible();
  // Signed in: the sign-in screen is gone.
  await expect(page.getByTestId('onboarding-sign-in')).toHaveCount(0);
});
