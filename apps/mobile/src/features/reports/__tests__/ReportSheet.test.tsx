import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { screen, userEvent, waitFor } from '@testing-library/react-native';
import { useState } from 'react';

import type { ColorScheme } from '@card-sharing/tokens';

import { t } from '../../../i18n';
import { FakeAuthServer, json, problem, type RecordedRequest } from '../../../test/fakeAuthServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { renderThemed, SCHEMES } from '../../../test/render';
import { Button } from '../../../ui';
import { AuthProvider } from '../../auth/AuthProvider';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { createReportsApi, ReportsError, type ReportTarget } from '../reportsApi';
import { ReportSheet } from '../ReportSheet';

const CARD = '0192f0dd-0000-7000-8000-000000000001';
const VISIBLE = new Set([CARD]);

/** `POST /v1/reports` as apps/api/src/routes/reports.rs answers it (T-141). */
function reportsServer() {
  const server = new FakeAuthServer();
  const reports: RecordedRequest[] = [];
  const keys = new Set<string>();
  server.routes = (request) => {
    if (request.method !== 'POST' || request.path !== '/v1/reports') return null;
    if (!request.idempotencyKey) return problem(400, 'VALIDATION_FAILED');
    const body = request.body as { target?: { type?: string; id?: string }; reason?: string };
    if (!body.target?.id || !VISIBLE.has(body.target.id)) return problem(404, 'NOT_FOUND');
    if (!keys.has(request.idempotencyKey)) reports.push(request);
    keys.add(request.idempotencyKey);
    return json(
      {
        report: {
          id: '0192f0ee-0000-7000-8000-000000000001',
          status: 'received',
          created_at: '2026-11-05T10:00:00Z',
        },
      },
      202,
    );
  };
  return { server, reports };
}

async function setup(target: ReportTarget, scheme: ColorScheme = 'light') {
  const { server, reports } = reportsServer();
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  const api = createReportsApi(client.fetch);
  function Harness() {
    const [open, setOpen] = useState<ReportTarget | null>(null);
    return (
      <>
        <Button label={t('report.open')} onPress={() => setOpen(target)} />
        <ReportSheet api={api} target={open} onClose={() => setOpen(null)} />
      </>
    );
  }
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: 0 } },
  });
  await renderThemed(
    <QueryClientProvider client={queryClient}>
      <AuthProvider client={client}>
        <Harness />
      </AuthProvider>
    </QueryClientProvider>,
    { scheme },
  );
  return { server, reports, api, user: userEvent.setup() };
}

describe('ReportSheet (T-141, AC-26.1)', () => {
  it.each(SCHEMES)('reports in two taps: Report, then a reason (%s)', async (scheme) => {
    const { reports, user } = await setup({ kind: 'card', id: CARD }, scheme);
    await user.press(screen.getByRole('button', { name: t('report.open') }));
    for (const key of [
      'report.reason.sexual',
      'report.reason.minorSafety',
      'report.reason.harassment',
      'report.reason.violence',
      'report.reason.hate',
      'report.reason.spam',
      'report.reason.other',
    ] as const) {
      expect(await screen.findByRole('button', { name: t(key) })).toBeTruthy();
    }
    await user.press(screen.getByRole('button', { name: t('report.reason.harassment') }));
    expect(await screen.findByText(t('report.sent'))).toBeTruthy();
    expect(reports).toHaveLength(1);
    expect(reports[0]?.body).toEqual({ target: { type: 'card', id: CARD }, reason: 'harassment' });
    expect(reports[0]?.idempotencyKey).toMatch(/^[0-9a-f-]{36}$/);
    await waitFor(() => expect(screen.queryByText(t('report.intro'))).toBeNull());
  });

  it('says a target I can no longer see is gone (404) and closes', async () => {
    const { reports, user } = await setup({
      kind: 'user',
      id: '0192f0dd-0000-7000-8000-0000000000ff',
    });
    await user.press(screen.getByRole('button', { name: t('report.open') }));
    await user.press(await screen.findByRole('button', { name: t('report.reason.spam') }));
    expect(await screen.findByText(t('report.error.gone'))).toBeTruthy();
    expect(reports).toHaveLength(0);
  });

  it('maps the daily limit', async () => {
    const { server, user } = await setup({ kind: 'card', id: CARD });
    const routes = server.routes;
    server.routes = (r) =>
      r.path === '/v1/reports' ? problem(429, 'RATE_LIMITED') : (routes?.(r) ?? null);
    await user.press(screen.getByRole('button', { name: t('report.open') }));
    await user.press(await screen.findByRole('button', { name: t('report.reason.other') }));
    expect(await screen.findByText(t('report.error.rateLimited'))).toBeTruthy();
  });

  it('never sends a malformed target id', async () => {
    const { server, api } = await setup({ kind: 'card', id: CARD });
    await expect(api.report({ kind: 'board', id: '../me' }, 'spam', 'k')).rejects.toEqual(
      new ReportsError('not_found'),
    );
    await expect(
      api.report({ kind: 'public_link', id: 'short' }, 'spam', 'k'),
    ).rejects.toBeInstanceOf(ReportsError);
    expect(server.count('POST', '/v1/reports')).toBe(0);
  });
});
