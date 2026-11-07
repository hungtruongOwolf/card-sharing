import { NAMES, expect, test } from '../support/app';
import { LIVE_TIMEOUT, activityIds, feedCard, setReaction } from '../support/flows';

test('a reaction reaches the card owner live, without a reload', async ({ app }) => {
  const bob = await app.signIn('bob');
  const alice = await app.signIn('alice');

  // Bob keeps his activity open; it must update from the live event stream (GET /v1/events, ADR-0010).
  await bob.page.getByRole('button', { name: /^Activity/ }).click();
  await expect(bob.page.getByTestId('activity')).toBeVisible();
  await expect(bob.page.getByTestId('activity-error')).toHaveCount(0);
  const before = new Set(await activityIds(bob.page));

  // Alice reacts to Bob's card (seeded "Nap time").
  await setReaction(alice.page, feedCard(alice.page, 'Nap time'), 'Heart');

  await expect
    .poll(
      async () => {
        const fresh = (await activityIds(bob.page)).filter((id) => !before.has(id));
        for (const id of fresh) {
          const text = await bob.page.getByTestId(id).innerText();
          if (text.includes(`${NAMES.alice} reacted to your card`)) return true;
        }
        return false;
      },
      { timeout: LIVE_TIMEOUT, message: 'Bob’s activity shows Alice’s reaction without a reload' },
    )
    .toBe(true);
});
