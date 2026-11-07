import { NAMES, expect, test } from '../support/app';
import {
  LIVE_TIMEOUT,
  addCaption,
  feedCard,
  importPhoto,
  sendAndWaitDelivered,
  uniqueCaption,
} from '../support/flows';

test('a reply with a card goes to the original sender only, live', async ({ app }) => {
  const alice = await app.signIn('alice');
  const bob = await app.signIn('bob');
  const caption = uniqueCaption('E2E reply');

  // Bob opens Alice's seeded card "Breakfast thief" and replies with a card (ADR-0010).
  await feedCard(bob.page, 'Breakfast thief')
    .getByTestId(/^feed-open-/)
    .click();
  await expect(bob.page.getByTestId('card-viewer')).toBeVisible();
  await bob.page.getByTestId('card-viewer-reply').click();
  await importPhoto(bob.page);
  await addCaption(bob.page, caption);
  await bob.page.getByRole('button', { name: 'Next', exact: true }).click();
  await expect(bob.page.getByTestId('send')).toBeVisible();
  // A reply has no audience picker: it goes to the sender of the card only.
  await expect(bob.page.getByTestId('send-reply')).toContainText(NAMES.alice);
  await expect(bob.page.getByLabel('Choose friends', { exact: true })).toHaveCount(0);
  await sendAndWaitDelivered(bob.page);

  const reply = feedCard(alice.page, caption);
  await expect(reply).toBeVisible({ timeout: LIVE_TIMEOUT });
  await expect(reply.getByText(NAMES.bob, { exact: true })).toBeVisible();
});
