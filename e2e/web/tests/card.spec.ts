import { NAMES, expect, test } from '../support/app';
import {
  LIVE_TIMEOUT,
  addCaption,
  feedCard,
  importPhoto,
  nextToSend,
  openEditorTool,
  sendAndWaitDelivered,
  setReaction,
  uniqueCaption,
} from '../support/flows';

test('an imported photo opens the editor with its cut-out subject and the AI entry', async ({
  app,
}) => {
  const { page } = await app.signIn('alice');
  await page.getByTestId('home-capture').click();
  await importPhoto(page);
  // The (stubbed) cutout found a subject: the Shape tray offers the whole-photo switch.
  const tray = await openEditorTool(page, 'Shape');
  await expect(tray.getByLabel('Use whole photo').first()).toBeVisible();
  await expect(tray.locator('[data-testid^="tile-"]').first()).toBeVisible();
  // An adult in a region with AI effects (fake provider in CI) sees the AI entry; the minor test checks its absence.
  await expect(page.getByTestId('editor-ai')).toBeVisible();
});

test.fixme('B1: a styled card from an imported photo reaches a friend live', async ({ app }) => {
  const bob = await app.signIn('bob');
  const alice = await app.signIn('alice');
  const caption = uniqueCaption('E2E card');

  await alice.page.getByTestId('home-capture').click();
  await importPhoto(alice.page);
  const tray = await openEditorTool(alice.page, 'Shape');
  // B1: on the web every tile says "unavailable: Needs a newer system version", so there is no card and no Next.
  const usable = tray.locator('[data-testid^="tile-"]:not([aria-label*=", unavailable:"])');
  await expect(usable.first()).toBeVisible();
  await usable.first().click();
  await addCaption(alice.page, caption);
  await nextToSend(alice.page);
  await alice.page.getByLabel('Choose friends', { exact: true }).click();
  await alice.page.getByLabel(`Send to ${NAMES.bob}`, { exact: true }).click();
  await sendAndWaitDelivered(alice.page);
  await expect(alice.page.getByTestId('home-capture')).toBeVisible();

  // Bob never reloads: the card arrives through the live event stream.
  const arrived = feedCard(bob.page, caption);
  await expect(arrived).toBeVisible({ timeout: LIVE_TIMEOUT });
  await expect(arrived.getByText(NAMES.alice, { exact: true })).toBeVisible();
  await expect(arrived.getByTestId(/^feed-image-/)).toBeVisible();
});

test('E1: the owner can export their card as an image on the web', async ({ app }) => {
  const alice = await app.signIn('alice');
  const bob = await app.signIn('bob');
  // The feed lists cards delivered to me, never my own, so Bob reaches his own card ("Nap time") from his activity
  // once Alice reacts to it.
  await setReaction(alice.page, feedCard(alice.page, 'Nap time'), 'Heart');
  await bob.page.getByRole('button', { name: /^Activity/ }).click();
  await expect(bob.page.getByTestId('activity')).toBeVisible();
  const reacted = bob.page
    .getByTestId(/^activity-[0-9a-f]{8}-/)
    .filter({ hasText: `${NAMES.alice} reacted to your card` })
    .first();
  await expect(reacted).toBeVisible({ timeout: LIVE_TIMEOUT });
  await reacted.click();
  await expect(bob.page.getByTestId('card-viewer-card')).toBeVisible();
  // E1: `canShareImages()` used to be true on iOS only, so the web viewer had no export button.
  await bob.page.getByRole('button', { name: 'Export image', exact: true }).click();
  // How the web hands the PNG over (download or Web Share) is the fix's choice; it must not fail.
  await bob.page.waitForTimeout(3_000);
  await expect(bob.page.getByText('Couldn’t export this card. Try again.')).toHaveCount(0);
});
