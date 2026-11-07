import { defineConfig } from '@playwright/test';

/**
 * Browser E2E suite for the app's web target (sprint 10 phase 0, testing-strategy.md "E2E").
 *
 * It drives the Expo web export against a real API + worker (dev sign-in, fake AI provider, fake scanners). CI starts
 * the stack and sets `E2E_SERVE_EXPORT=1` so this config serves `apps/mobile/dist` (serve.mjs). Locally, point
 * `E2E_BASE_URL` at a running web app (e.g. `pnpm dev:mobile` with web) and `E2E_API_URL` at the API.
 *
 * Tests share one seeded database and per-user rate limits, so they run one at a time.
 */
const baseURL = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:8081';
const serveExport = process.env.E2E_SERVE_EXPORT === '1';
const ci = Boolean(process.env.CI);

export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  workers: 1,
  forbidOnly: ci,
  // One retry in CI keeps a single slow runner from blocking merges; a test that needed it is reported as flaky
  // and must be fixed or quarantined within the sprint (testing-strategy.md).
  retries: ci ? 1 : 0,
  timeout: 180_000,
  expect: { timeout: 15_000 },
  reporter: ci
    ? [['list'], ['github'], ['html', { open: 'never', outputFolder: 'playwright-report' }]]
    : [['list']],
  outputDir: 'test-results',
  use: {
    baseURL,
    viewport: { width: 390, height: 844 },
    locale: 'en-US',
    timezoneId: 'UTC',
    actionTimeout: 20_000,
    navigationTimeout: 60_000,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'off',
    // CanvasKit needs WebGL; on a GPU-less runner Chromium renders it with SwiftShader (trusted test content only).
    launchOptions: { args: ['--enable-unsafe-swiftshader'] },
  },
  projects: [{ name: 'chromium', use: { browserName: 'chromium' } }],
  webServer: serveExport
    ? {
        command: 'node serve.mjs',
        url: baseURL,
        reuseExistingServer: false,
        timeout: 30_000,
      }
    : undefined,
});
