import type { Page, Response } from '@playwright/test';

/** Something that fails the test: a page error, a console error, an HTTP 5xx or an unexpected 4xx. */
export interface Issue {
  readonly user: string;
  readonly kind: string;
  readonly text: string;
}

/** A 4xx a test expects (a deliberate negative step); everything else in 4xx fails the test. */
export interface ExpectedHttp {
  readonly method: string;
  readonly path: RegExp;
  readonly status: number;
}

/**
 * Console errors that are not failures. Keep this list short; every entry needs a reason.
 *
 * - Chromium's own "Failed to load resource: the server responded with a status of N" line repeats an HTTP response
 *   the response check below already judged (5xx always fails, 4xx fails unless the test expects it), so it is not
 *   judged twice.
 */
const IGNORED_CONSOLE_ERRORS: readonly RegExp[] = [
  /^Failed to load resource: the server responded with a status of \d{3}\b/,
];

/**
 * Watches the pages of one test and collects every {@link Issue}. Aborted requests (navigation, closing a page,
 * the live events stream ending) are logged for the report but do not fail the test.
 */
export class Guard {
  readonly issues: Issue[] = [];
  readonly log: string[] = [];
  private readonly expected: ExpectedHttp[] = [];

  expectHttp(expected: ExpectedHttp): void {
    this.expected.push(expected);
  }

  watch(page: Page, user: string): void {
    const started = Date.now();
    const note = (kind: string, text: string) =>
      this.log.push(`+${((Date.now() - started) / 1000).toFixed(1)}s [${user}] ${kind}: ${text}`);

    page.on('pageerror', (error) => {
      const text = error.stack ?? error.message;
      note('pageerror', text);
      this.issues.push({ user, kind: 'pageerror', text: firstLines(text) });
    });
    page.on('console', (message) => {
      if (message.type() !== 'error' && message.type() !== 'warning') return;
      const text = message.text();
      note(`console.${message.type()}`, text);
      if (message.type() !== 'error') return;
      if (IGNORED_CONSOLE_ERRORS.some((pattern) => pattern.test(text))) return;
      this.issues.push({ user, kind: 'console.error', text: firstLines(text) });
    });
    page.on('requestfailed', (request) => {
      note(
        'requestfailed',
        `${request.method()} ${request.url()} ${request.failure()?.errorText ?? ''}`,
      );
    });
    page.on('response', (response) => this.judge(user, response, note));
  }

  private judge(
    user: string,
    response: Response,
    note: (kind: string, text: string) => void,
  ): void {
    const status = response.status();
    if (status < 400) return;
    const method = response.request().method();
    const { pathname } = new URL(response.url());
    const text = `${method} ${pathname} -> ${status}`;
    note('http', text);
    if (
      status < 500 &&
      this.expected.some((e) => e.status === status && e.method === method && e.path.test(pathname))
    ) {
      return;
    }
    this.issues.push({ user, kind: status >= 500 ? 'http.5xx' : 'http.4xx', text });
  }
}

function firstLines(text: string): string {
  return text.split('\n').slice(0, 3).join('\n').slice(0, 500);
}
