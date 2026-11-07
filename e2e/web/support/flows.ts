import type { Locator, Page } from '@playwright/test';

import { expect } from './app';
import { photoPng } from './photo';

/** How long a live update (ADR-0010 event stream → refetch) may take to show on another user's screen. */
export const LIVE_TIMEOUT = 30_000;

/** A caption no other test or earlier run has used. */
export function uniqueCaption(prefix: string): string {
  return `${prefix} ${Date.now().toString(36)}`;
}

/** The feed tile whose sender line or caption contains `text`. */
export function feedCard(page: Page, text: string): Locator {
  return page.getByTestId(/^feed-card-/).filter({ hasText: text });
}

/** Opens an entry of the home screen's More sheet. */
export async function openMore(page: Page, item: string): Promise<void> {
  await page.getByRole('button', { name: 'More', exact: true }).click();
  await page.getByRole('button', { name: item, exact: true }).click();
}

/**
 * From the capture screen: imports the generated photo through the browser's file chooser and opens the editor.
 * The cutout segmenter is stubbed per context (support/cutout.ts), so the editor gets a subject at once.
 */
export async function importPhoto(page: Page): Promise<void> {
  await expect(page.getByTestId('capture')).toBeVisible();
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', { name: 'Choose from your photos', exact: true }).click();
  await (await chooser).setFiles({ name: 'ball.png', mimeType: 'image/png', buffer: photoPng() });
  await expect(page.getByTestId('capture-preview')).toBeVisible();
  await page.getByRole('button', { name: 'Use this photo', exact: true }).click();
  await expect(page.getByTestId('editor')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Next', exact: true })).toBeVisible({
    timeout: 30_000,
  });
}

export async function addCaption(page: Page, caption: string): Promise<void> {
  await page.getByRole('button', { name: 'Add caption', exact: true }).click();
  await page.getByTestId('caption-input').fill(caption);
  await page.getByRole('button', { name: 'Save caption', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Edit caption', exact: true })).toBeVisible();
}

/** From the send screen: waits until the server delivered the card (scan passed), then closes the status. */
export async function sendAndWaitDelivered(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(page.getByTestId('send-status-active')).toBeVisible({ timeout: 60_000 });
  await page.getByRole('button', { name: 'Done', exact: true }).click();
}

/**
 * Taps a reaction on a card and makes sure it ends up set (a tap on the chosen reaction removes it; then tap again),
 * so the owner gets a fresh reaction event.
 */
export async function setReaction(page: Page, card: Locator, kind: string): Promise<void> {
  for (let attempt = 0; attempt < 2; attempt++) {
    const response = page.waitForResponse(
      (r) =>
        /^\/v1\/cards\/[^/]+\/reaction$/.test(new URL(r.url()).pathname) &&
        (r.request().method() === 'PUT' || r.request().method() === 'DELETE'),
    );
    await card.getByRole('button', { name: kind, exact: true }).click();
    const answered = await response;
    expect(answered.ok(), `${answered.request().method()} reaction -> ${answered.status()}`).toBe(
      true,
    );
    if (answered.request().method() === 'PUT') return;
  }
  throw new Error(`the ${kind} reaction was removed twice instead of being set`);
}

/** Activity rows (`activity-<uuid>`) currently listed. */
export async function activityIds(page: Page): Promise<string[]> {
  return page
    .getByTestId(/^activity-[0-9a-f]{8}-/)
    .evaluateAll((rows) => rows.map((row) => row.getAttribute('data-testid') ?? ''));
}

/** Shows an editor tool's tray. The tray opens on the first tool and a tap on the shown tool closes it. */
export async function openEditorTool(page: Page, label: string): Promise<Locator> {
  const tray = page.getByTestId('editor-tray');
  await expect(tray).toBeVisible();
  const heading = tray.getByRole('heading', { name: label, exact: true });
  if (!(await heading.isVisible())) {
    await page.getByLabel(label, { exact: true }).click();
    await expect(heading).toBeVisible();
  }
  return tray;
}
