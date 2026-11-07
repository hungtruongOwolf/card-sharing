import type { Page } from '@playwright/test';

import { expect, test } from '../support/app';

/** Access tokens live 10 minutes (ADR-0004); past that the app refreshes before its next call. */
const PAST_ACCESS_TTL = '10:30';
/** The server accepts a just-rotated refresh token again for 30 s (reuse grace); wait past it. */
const PAST_REUSE_GRACE_MS = 35_000;

async function openActivityAndBack(page: Page): Promise<void> {
  await page.getByRole('button', { name: /^Activity/ }).click();
  await expect(page.getByTestId('activity')).toBeVisible();
  await expect(page.getByTestId('activity-error')).toHaveCount(0);
  await page.getByRole('button', { name: 'Back', exact: true }).click();
  await expect(page.getByTestId('home-capture')).toBeVisible();
}

test('B5: two tabs of one user stay signed in past the access-token lifetime', async ({ app }) => {
  const alice = await app.signIn('alice', { clock: true });
  const tab1 = alice.page;
  const tab2 = await alice.newTab();
  await expect(tab2.getByTestId('home-capture')).toBeVisible({ timeout: 30_000 });

  // Tab 1's access token expires first: it refreshes and rotates the refresh token.
  await tab1.clock.fastForward(PAST_ACCESS_TTL);
  await openActivityAndBack(tab1);

  // Tab 2 loaded the old pair; past the reuse grace, refreshing with it would revoke the whole session family (B5)
  // unless the tabs coordinate (Web Lock + re-read of the shared store + storage events).
  await tab2.waitForTimeout(PAST_REUSE_GRACE_MS);
  await tab2.clock.fastForward(PAST_ACCESS_TTL);
  await openActivityAndBack(tab2);

  // Both tabs are still signed in afterwards.
  await openActivityAndBack(tab1);
  for (const tab of [tab1, tab2]) {
    await expect(tab.getByTestId('onboarding-sign-in')).toHaveCount(0);
    await expect(tab.getByTestId('home-capture')).toBeVisible();
  }
});
