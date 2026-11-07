import type { Page } from '@playwright/test';

import { NAMES, expect, test } from '../support/app';
import {
  LIVE_TIMEOUT,
  addCaption,
  feedCard,
  importPhoto,
  nextToSend,
  openEditorTool,
  sendAndWaitDelivered,
  uniqueCaption,
} from '../support/flows';

/** Bob opens Alice's seeded card "Breakfast thief", taps Reply and turns a photo into a card (ADR-0010). */
async function startReply(bob: Page): Promise<void> {
  await feedCard(bob, 'Breakfast thief')
    .getByTestId(/^feed-open-/)
    .click();
  await expect(bob.getByTestId('card-viewer')).toBeVisible();
  await bob
    .getByRole('button', { name: `Reply to ${NAMES.alice} with a card`, exact: true })
    .click();
  await importPhoto(bob);
}

test('a reply with a card starts from the friend’s card', async ({ app }) => {
  const bob = await app.signIn('bob');
  await startReply(bob.page);
  await openEditorTool(bob.page, 'Shape');
});

test.fixme('B1: a reply with a card goes to the original sender only, live', async ({ app }) => {
  const alice = await app.signIn('alice');
  const bob = await app.signIn('bob');
  const caption = uniqueCaption('E2E reply');

  await startReply(bob.page);
  // B1: no usable style on the web, so the editor has no card and no Next.
  await addCaption(bob.page, caption);
  await nextToSend(bob.page);
  // A reply has no audience picker: it goes to the sender of the card only.
  await expect(bob.page.getByTestId('send-reply')).toContainText(NAMES.alice);
  await expect(bob.page.getByLabel('Choose friends', { exact: true })).toHaveCount(0);
  await sendAndWaitDelivered(bob.page);

  const reply = feedCard(alice.page, caption);
  await expect(reply).toBeVisible({ timeout: LIVE_TIMEOUT });
  await expect(reply.getByText(NAMES.bob, { exact: true })).toBeVisible();
});
