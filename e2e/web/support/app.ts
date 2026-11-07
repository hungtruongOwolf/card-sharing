import {
  expect,
  test as base,
  type Browser,
  type BrowserContext,
  type Page,
  type TestInfo,
} from '@playwright/test';

import { stubCutoutWorker } from './cutout';
import { Guard, type ExpectedHttp } from './guard';

/** The seed users (`apps/api/src/seed.rs`): Alice and Bob are adults and friends; Mia is a minor, Alice's friend. */
export type Persona = 'alice' | 'bob' | 'mia';

export const NAMES: Record<Persona, string> = {
  alice: 'Alice (demo)',
  bob: 'Bob (demo)',
  mia: 'Mia (demo, minor)',
};

export interface OpenOptions {
  /** Installs Playwright's clock before the first page loads, so a test can move this user's time forward. */
  clock?: boolean;
}

/** One browser profile of one user (its own storage, so its own session) and its first tab. */
export class Session {
  constructor(
    readonly persona: Persona,
    readonly context: BrowserContext,
    readonly page: Page,
    private readonly guard: Guard,
  ) {}

  /** Another tab of the same profile (shares the stored session). */
  async newTab(): Promise<Page> {
    const page = await this.context.newPage();
    this.guard.watch(page, `${this.persona}#${this.context.pages().length}`);
    await page.goto('/');
    return page;
  }
}

/**
 * Per-test entry point: opens users in separate browser contexts, watches every page for page errors, console
 * errors, HTTP 5xx and unexpected 4xx, and fails the test on any of them after it ran.
 */
export class App {
  private readonly guard = new Guard();
  private readonly contexts: BrowserContext[] = [];

  constructor(private readonly browser: Browser) {}

  /** Declares a 4xx this test causes on purpose. */
  expectHttp(expected: ExpectedHttp): void {
    this.guard.expectHttp(expected);
  }

  /** A signed-out user on the app's first screen. */
  async open(persona: Persona, options: OpenOptions = {}): Promise<Session> {
    const context = await this.browser.newContext();
    this.contexts.push(context);
    await stubCutoutWorker(context);
    if (options.clock) await context.clock.install();
    const page = await context.newPage();
    this.guard.watch(page, persona);
    await page.goto('/');
    return new Session(persona, context, page, this.guard);
  }

  /** A user signed in with the development sign-in (#306), on the home feed. */
  async signIn(persona: Persona, options: OpenOptions = {}): Promise<Session> {
    const session = await this.open(persona, options);
    await signInAsDemoUser(session.page, persona);
    return session;
  }

  async finish(testInfo: TestInfo): Promise<void> {
    for (const context of this.contexts) await context.close().catch(() => undefined);
    if (this.guard.log.length > 0) {
      await testInfo.attach('browser-log.txt', {
        body: this.guard.log.join('\n'),
        contentType: 'text/plain',
      });
    }
    expect(this.guard.issues, 'page errors, console errors, HTTP 5xx and unexpected 4xx').toEqual(
      [],
    );
  }
}

export async function signInAsDemoUser(page: Page, persona: Persona): Promise<void> {
  await page.getByRole('button', { name: 'Sign in as demo user', exact: true }).click();
  await page.getByRole('button', { name: new RegExp(`\\(@${persona}\\.demo\\)`) }).click();
  await expect(page.getByTestId('home-capture')).toBeVisible({ timeout: 30_000 });
}

export const test = base.extend<{ app: App }>({
  app: async ({ browser }, use, testInfo) => {
    const app = new App(browser);
    await use(app);
    await app.finish(testInfo);
  },
});

export { expect };
