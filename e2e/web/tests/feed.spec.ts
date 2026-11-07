import { expect, test } from '../support/app';
import { feedCard } from '../support/flows';

test('the feed lists friends’ cards and the story viewer opens and closes', async ({ app }) => {
  const { page } = await app.signIn('alice');

  // Seeded: Bob answered Alice's first card with "Nap time".
  const napTime = feedCard(page, 'Nap time');
  await expect(napTime).toBeVisible();
  await expect(napTime.getByTestId(/^feed-image-/)).toBeVisible();

  // A story ring opens that friend's cards as a story.
  await page
    .getByTestId(/^story-ring-/)
    .first()
    .click();
  await expect(page.getByTestId('card-viewer')).toBeVisible();
  await expect(page.getByTestId('card-viewer-image')).toBeVisible({ timeout: 30_000 });
  await expect(page.getByTestId('card-viewer-error')).toHaveCount(0);
  await page.getByTestId('card-viewer-close').click();
  await expect(page.getByTestId('card-viewer')).toHaveCount(0);
  await expect(page.getByTestId('home-capture')).toBeVisible();

  // A feed tile opens that card full screen, with its caption and reactions.
  await napTime.getByTestId(/^feed-open-/).click();
  const viewer = page.getByTestId('card-viewer');
  await expect(viewer).toBeVisible();
  await expect(page.getByTestId('card-viewer-image')).toBeVisible({ timeout: 30_000 });
  await expect(viewer.getByText('Nap time', { exact: true })).toBeVisible();
  await expect(viewer.getByRole('button', { name: 'Heart', exact: true })).toBeVisible();
  await page.getByTestId('card-viewer-close').click();
  await expect(page.getByTestId('home-capture')).toBeVisible();
});
