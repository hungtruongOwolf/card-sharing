import { expect, test } from '../support/app';
import { importPhoto } from '../support/flows';

test('a minor’s editor has no AI effects entry', async ({ app }) => {
  const { page } = await app.signIn('mia');
  // The editor shows the AI entry from the server's answer (`available`), so wait for that answer first.
  const overview = page.waitForResponse(
    (r) => new URL(r.url()).pathname === '/v1/ai-effects' && r.request().method() === 'GET',
  );
  await page.getByTestId('home-capture').click();
  await importPhoto(page);
  const answer = await overview;
  expect(answer.status()).toBe(200);
  expect((await answer.json()) as { available?: unknown }).toMatchObject({ available: false });

  await expect(page.getByLabel('Stickers', { exact: true })).toBeVisible();
  await expect(page.getByTestId('editor-ai')).toHaveCount(0);
  await expect(page.getByLabel('AI effects', { exact: true })).toHaveCount(0);
});
