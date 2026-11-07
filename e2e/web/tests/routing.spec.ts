import { expect, test } from '../support/app';
import { feedCard } from '../support/flows';

test.fixme('R1: a deep link and a reload keep the screen', async ({ app }) => {
  const { page } = await app.signIn('alice');

  await page.goto('/boards');
  await expect(page.getByTestId('boards')).toBeVisible();
  await page.reload();
  await expect(page.getByTestId('boards')).toBeVisible();

  await page.goto('/settings/privacy');
  await expect(page.getByTestId('privacy')).toBeVisible();
  await page.reload();
  await expect(page.getByTestId('privacy')).toBeVisible();
});

test.fixme('R1: a card link opens the card after a reload', async ({ app }) => {
  const { page } = await app.signIn('alice');
  await feedCard(page, 'Nap time')
    .getByTestId(/^feed-open-/)
    .click();
  await expect(page.getByTestId('card-viewer')).toBeVisible();
  await expect(page).toHaveURL(/\/card\/[0-9a-f-]{36}$/);
  await page.reload();
  const viewer = page.getByTestId('card-viewer');
  await expect(viewer).toBeVisible();
  await expect(viewer.getByText('Nap time', { exact: true })).toBeVisible();
});
