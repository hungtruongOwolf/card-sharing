import type { Page } from '@playwright/test';

import { NAMES, expect, test } from '../support/app';
import { signInLink } from '../support/mailpit';

/**
 * Real sign-up on the web (sprint 10 phase 2, ADR-0012): an email magic link (Mailpit catches the mail), the invite-code
 * gate (`config/signup.yaml` is on; CI mints a code with `api invites create` into E2E_INVITE_CODE), the onboarding
 * steps, and the link failure paths. Every run uses fresh addresses; at most two links per run, inside the
 * `auth.email.start` limit of 5 per 15 minutes per IP even with a retry.
 */

function inviteCode(): string {
  const code = process.env.E2E_INVITE_CODE;
  if (!code) throw new Error('E2E_INVITE_CODE is not set (mint one: api invites create --uses 20)');
  return code;
}

const unique = () => `${Date.now().toString(36)}${Math.floor(Math.random() * 1296).toString(36)}`;

async function askForLink(page: Page, email: string): Promise<void> {
  await page.getByTestId('sign-in-email').fill(email);
  await page.getByRole('button', { name: 'Email me a sign-in link', exact: true }).click();
  await expect(page.getByTestId('check-email')).toBeVisible();
}

test('a new user signs up by magic link with an invite code, onboards and adds alice by handle', async ({
  app,
}) => {
  const id = unique();
  const email = `newcomer-${id}@e2e.test`;
  const user = await app.open('newcomer');
  await askForLink(user.page, email);

  // The emailed link opens in a new tab of the same browser, which holds the PKCE verifier.
  const link = await signInLink(email);
  expect(new URL(link).pathname).toBe('/auth/link');
  const tab = await user.openTab(link);
  // The landing route is a lazy bundle in Metro's development mode: the first visit compiles it.
  await expect(tab.getByTestId('magic-link-confirm')).toBeVisible({ timeout: 60_000 });
  // The token is read once and removed from the address bar.
  await expect.poll(() => tab.url()).not.toContain('token=');

  // A new account needs an invite code: none sent yet, then a wrong one (both deliberate 403s), then the real one.
  app.expectHttp({ method: 'POST', path: /^\/v1\/auth\/email\/verify$/, status: 403 });
  await tab.getByRole('button', { name: 'Continue', exact: true }).click();
  await expect(tab.getByTestId('magic-link-invite')).toBeVisible();
  await tab.getByTestId('invite-code').fill('ZZZZ-ZZZZ-ZZZZ');
  await tab.getByRole('button', { name: 'Join', exact: true }).click();
  await expect(
    tab.getByText('That code doesn’t work. Check it, or ask for a new one.'),
  ).toBeVisible();
  await tab.getByTestId('invite-code').fill(inviteCode());
  await tab.getByRole('button', { name: 'Join', exact: true }).click();

  // Onboarding: date of birth (an adult), display name, handle; the web has no camera prompt.
  await expect(tab.getByTestId('onboarding-birth-date')).toBeVisible({ timeout: 30_000 });
  await tab.getByTestId('birth-date-day').fill('15');
  await tab.getByTestId('birth-date-month').fill('06');
  await tab.getByTestId('birth-date-year').fill('1995');
  await tab.getByRole('button', { name: 'Continue', exact: true }).click();
  await expect(tab.getByTestId('onboarding-display-name')).toBeVisible();
  await tab.getByTestId('display-name').fill(`Newcomer ${id}`);
  await tab.getByRole('button', { name: 'Continue', exact: true }).click();
  await expect(tab.getByTestId('onboarding-handle')).toBeVisible();
  await tab.getByTestId('handle').fill(`nc${id}`.slice(0, 20));
  await tab.getByRole('button', { name: 'Continue', exact: true }).click();
  await expect(tab.getByTestId('home-capture')).toBeVisible({ timeout: 30_000 });

  // The tab that asked for the link follows the sign-in (shared storage, B5).
  await expect(user.page.getByTestId('onboarding-sign-in')).toHaveCount(0, { timeout: 30_000 });

  // Find alice by handle and send her a friend request.
  await tab.getByRole('button', { name: 'Friends', exact: true }).click();
  await tab.getByRole('button', { name: 'Find by handle', exact: true }).click();
  await expect(tab.getByTestId('find-friend')).toBeVisible();
  await tab.getByRole('textbox').first().fill('alice.demo');
  await tab.getByRole('button', { name: 'Search', exact: true }).click();
  await expect(tab.getByTestId('find-result')).toContainText(NAMES.alice);
  await tab.getByTestId('find-result').getByRole('button').click();
  await expect(tab.getByTestId('find-sent')).toBeVisible();
});

test('sign-in links fail safely: another browser, an expired link, a fresh start', async ({
  app,
}) => {
  const email = `linkcheck-${unique()}@e2e.test`;
  const user = await app.open('linkcheck');
  await askForLink(user.page, email);
  const link = await signInLink(email);
  const requestId = /#rid=([0-9a-f-]{36})&/.exec(link)?.[1] ?? '';

  // Another browser has no verifier for the link: it says where to open it and never calls the server.
  const other = await app.open('other-browser');
  const verifies: string[] = [];
  other.page.on('request', (request) => {
    if (request.url().includes('/v1/auth/email/verify')) verifies.push(request.url());
  });
  await other.page.goto(link);
  await expect(other.page.getByTestId('magic-link-otherBrowser')).toBeVisible({ timeout: 60_000 });
  expect(verifies).toEqual([]);

  // In the browser that asked, the link is past its 15 minutes (the stored request is aged by 16).
  await user.page.evaluate((key) => {
    const raw = localStorage.getItem(key);
    if (!raw) throw new Error(`no stored request ${key}`);
    const record = JSON.parse(raw) as { createdAt: number };
    localStorage.setItem(
      key,
      JSON.stringify({ ...record, createdAt: record.createdAt - 16 * 60_000 }),
    );
  }, `cs.secure.cs.magiclink.v1.${requestId}`);
  const tab = await user.openTab(link);
  await expect(tab.getByTestId('magic-link-expired')).toBeVisible();

  // "Ask for a new link" goes back to the sign-in screen.
  await tab.getByRole('button', { name: 'Ask for a new link', exact: true }).click();
  await expect(tab.getByTestId('email-sign-in')).toBeVisible();
});
