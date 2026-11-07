import { NAMES, expect, test, type Session } from '../support/app';
import {
  LIVE_TIMEOUT,
  addCaption,
  feedCard,
  importPhoto,
  openEditorTool,
  sendAndWaitDelivered,
  uniqueCaption,
} from '../support/flows';

/** From the editor: Next, then send to Bob only. */
async function sendToBob(alice: Session): Promise<void> {
  const { page } = alice;
  await page.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(page.getByTestId('send')).toBeVisible();
  await page.getByLabel('Choose friends', { exact: true }).click();
  await page.getByLabel(`Send to ${NAMES.bob}`, { exact: true }).click();
  await sendAndWaitDelivered(page);
}

test('a card made from an imported photo reaches a friend live', async ({ app }) => {
  const bob = await app.signIn('bob');
  const alice = await app.signIn('alice');
  const caption = uniqueCaption('E2E ball');

  await alice.page.getByTestId('home-capture').click();
  await importPhoto(alice.page);
  // The (stubbed) cutout found a subject: the Shape tray offers the whole-photo switch.
  const tray = await openEditorTool(alice.page, 'Shape');
  await expect(tray.getByLabel('Use whole photo').first()).toBeVisible();
  // An adult in a region with AI effects (fake provider in CI) sees the AI entry; the minor test checks its absence.
  await expect(alice.page.getByTestId('editor-ai')).toBeVisible();
  await addCaption(alice.page, caption);
  await sendToBob(alice);
  await expect(alice.page.getByTestId('home-capture')).toBeVisible();

  // Bob never reloads: the card arrives through the live event stream.
  const arrived = feedCard(bob.page, caption);
  await expect(arrived).toBeVisible({ timeout: LIVE_TIMEOUT });
  await expect(arrived.getByText(NAMES.alice, { exact: true })).toBeVisible();
  await expect(arrived.getByTestId(/^feed-image-/)).toBeVisible();
});

test.fixme('B1: a card style picked in the editor is sent to a friend', async ({ app }) => {
  const bob = await app.signIn('bob');
  const alice = await app.signIn('alice');
  const caption = uniqueCaption('E2E styled');

  await alice.page.getByTestId('home-capture').click();
  await importPhoto(alice.page);
  const tray = await openEditorTool(alice.page, 'Shape');
  // B1: on the web every tile says "unavailable: Needs a newer system version".
  const usable = tray.locator('[data-testid^="tile-"]:not([aria-label*=", unavailable:"])');
  await expect(usable.first()).toBeVisible();
  await usable.first().click();
  await addCaption(alice.page, caption);
  await sendToBob(alice);
  await expect(feedCard(bob.page, caption)).toBeVisible({ timeout: LIVE_TIMEOUT });
});

test.fixme('E1: the owner can export their card as an image on the web', async ({ app }) => {
  const { page } = await app.signIn('alice');
  // Alice's own seeded card.
  await feedCard(page, 'Breakfast thief')
    .getByTestId(/^feed-open-/)
    .click();
  await expect(page.getByTestId('card-viewer-image')).toBeVisible({ timeout: 30_000 });
  // E1: `canShareImages()` is true on iOS only, so the web viewer has no export button.
  await page.getByRole('button', { name: 'Export image', exact: true }).click();
  // How the web hands the PNG over (download or Web Share) is the fix's choice; it must not fail.
  await page.waitForTimeout(3_000);
  await expect(page.getByText('Couldn’t export this card. Try again.')).toHaveCount(0);
});
