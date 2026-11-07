import type { Page } from '@playwright/test';

import { expect, test } from '../support/app';
import { openMore } from '../support/flows';

/** Pins counted by the board canvas label ("Board, 4 pins in view"). */
async function pinsInView(page: Page): Promise<number> {
  const label = (await page.getByTestId('board-canvas').getAttribute('aria-label')) ?? '';
  if (/no pins in view/.test(label)) return 0;
  const count = /(\d+) pins? in view/.exec(label)?.[1];
  if (count === undefined) throw new Error(`unexpected board canvas label: ${label}`);
  return Number(count);
}

async function openSharedBoard(page: Page): Promise<void> {
  await openMore(page, 'Boards');
  await expect(page.getByTestId('boards')).toBeVisible();
  // Seeded: Alice's board "Weekend scrapbook", shared with Bob.
  await page.getByRole('button', { name: /^Weekend scrapbook, / }).click();
  await expect(page.getByTestId('board')).toBeVisible();
  await expect(page.getByTestId('board-canvas')).toBeVisible();
}

test('a card is pinned to the shared board', async ({ app }) => {
  const { page } = await app.signIn('alice');
  await openSharedBoard(page);
  await expect.poll(() => pinsInView(page)).toBeGreaterThan(0);
  const before = await pinsInView(page);

  await page.getByRole('button', { name: 'Pin a card', exact: true }).click();
  const picker = page.getByTestId('pin-picker');
  await expect(picker).toBeVisible();
  await picker
    .getByRole('button', { name: /^(Card from|Your card)/ })
    .first()
    .click();
  await expect.poll(() => pinsInView(page), { timeout: 20_000 }).toBe(before + 1);
});

test('B3: the owner creates a public board link and sees its URL', async ({ app }) => {
  const { page } = await app.signIn('alice');
  await openSharedBoard(page);
  await page.getByRole('button', { name: 'Share link', exact: true }).first().click();
  const sheet = page.getByTestId('public-links');
  await expect(sheet).toBeVisible();
  // The one-time explanation shows first unless already seen; the expiry choices are radios.
  const gotIt = sheet.getByRole('button', { name: 'Got it', exact: true });
  const day = sheet.getByRole('radio', { name: '24 hours', exact: true });
  await expect(gotIt.or(day)).toBeVisible();
  if (await gotIt.isVisible()) await gotIt.click();
  await day.click();
  await sheet.getByRole('button', { name: 'Create link', exact: true }).click();
  // B3: the API answered 201 with a plain-http local URL and the sheet said it could not update the links.
  const created = sheet.getByTestId('public-links-created');
  await expect(created).toBeVisible();
  await expect(created).toContainText(/https?:\/\/\S+\/b\/[A-Za-z0-9_-]{16,}/);
  await expect(sheet.getByText('Couldn’t update links. Try again.')).toHaveCount(0);
});
