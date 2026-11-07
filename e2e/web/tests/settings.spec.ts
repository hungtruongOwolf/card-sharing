import { expect, test } from '../support/app';
import { openMore } from '../support/flows';

/** Every settings entry of the More sheet, with the screen it opens. */
const SCREENS = [
  ['Notifications', 'settings-notifications'],
  ['Privacy', 'privacy'],
  ['Sounds and haptics', 'feedback-settings'],
  ['Signed-in devices', 'settings-sessions'],
  ['Download your data', 'data-export'],
  ['Delete account', 'delete-account'],
] as const;

test('every settings screen opens without an error', async ({ app }) => {
  const { page } = await app.signIn('alice');
  for (const [entry, screen] of SCREENS) {
    await test.step(entry, async () => {
      await openMore(page, entry);
      await expect(page.getByTestId(screen)).toBeVisible();
      // Screens show a `<name>-error` block when a load fails; let their first requests finish.
      await page.waitForTimeout(1_500);
      await expect(page.locator('[data-testid$="-error"]')).toHaveCount(0);
      const back = page.getByRole('button', { name: 'Back', exact: true });
      if (await back.isVisible()) await back.click();
      else await page.goBack();
      await expect(page.getByTestId('home-capture')).toBeVisible();
    });
  }
});

test('the signed-in devices screen lists this session', async ({ app }) => {
  const { page } = await app.signIn('alice');
  await openMore(page, 'Signed-in devices');
  await expect(page.getByTestId('settings-sessions')).toBeVisible();
  await expect(page.locator('[data-testid^="session-"]').first()).toBeVisible();
});
