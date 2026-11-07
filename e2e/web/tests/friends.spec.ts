import { NAMES, expect, test } from '../support/app';

test('a friend is found by handle and the invite link is shown', async ({ app }) => {
  const { page } = await app.signIn('alice');

  await page.getByRole('button', { name: 'Friends', exact: true }).click();
  await expect(page.getByTestId('friends')).toBeVisible();
  await page.getByRole('button', { name: 'Find by handle', exact: true }).click();
  await expect(page.getByTestId('find-friend')).toBeVisible();
  await page.getByRole('textbox').first().fill('bob.demo');
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await expect(page.getByTestId('find-result')).toContainText(NAMES.bob);
  await page.getByRole('button', { name: 'Back', exact: true }).click();

  await expect(page.getByTestId('friends')).toBeVisible();
  await page.getByRole('button', { name: 'Invite friends', exact: true }).click();
  await expect(page.getByTestId('invite')).toBeVisible();
  await expect(page.getByTestId('invite-url')).toHaveText(
    /^https?:\/\/\S+\/i\/[A-Za-z0-9_-]{16,}$/,
  );
  await expect(page.getByTestId('qr-code')).toBeVisible();
});

test('R1: an invite link opens the invite in the web app for another user', async ({ app }) => {
  const alice = await app.signIn('alice');
  await alice.page.getByRole('button', { name: 'Friends', exact: true }).click();
  await alice.page.getByRole('button', { name: 'Invite friends', exact: true }).click();
  const url = (await alice.page.getByTestId('invite-url').innerText()).trim();
  const token = /\/i\/([A-Za-z0-9_-]+)$/.exec(url)?.[1];
  expect(token, url).toBeTruthy();

  // The app's own /i/<token> route (B4 points invite links at the web app); R1 used to land it on home.
  const bob = await app.signIn('bob');
  await bob.page.goto(`/i/${token}`);
  await expect(bob.page.getByTestId('invite-preview')).toBeVisible();
  await expect(bob.page.getByTestId('invite-preview')).toContainText(NAMES.alice);
});
