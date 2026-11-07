import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { FakeOnboardingServer } from '../../../test/fakeOnboardingServer';
import { json, problem } from '../../../test/fakeAuthServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { createOnboardingApi, OnboardingError } from '../onboardingApi';

function setup() {
  const server = new FakeOnboardingServer();
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.auth.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  return { server, api: createOnboardingApi(client.fetch) };
}

const kindOf = (promise: Promise<unknown>) =>
  promise.then(
    () => 'resolved',
    (err: unknown) => (err instanceof OnboardingError ? err.kind : String(err)),
  );

it('reads the step and submits the DOB as YYYY-MM-DD with the session token', async () => {
  const { server, api } = setup();
  expect(await api.getMe()).toEqual({
    step: 'age',
    deletionPending: false,
    publicLinksAllowed: true,
  });
  expect(await api.submitBirthDate('2000-01-31')).toBe('profile');
  const age = server.auth.requests.find((r) => r.path === '/v1/me/age');
  expect(age?.body).toEqual({ birth_date: '2000-01-31' });
  expect(age?.authorization).toMatch(/^Bearer access-/);
});

it('exposes only the step and the deletion flag from /v1/me, never the region threshold', async () => {
  const { api } = setup();
  const me = await api.getMe();
  expect(Object.keys(me)).toEqual(['step', 'deletionPending', 'publicLinksAllowed']);
});

it('reads the public-link hint and treats a missing one as unknown (#321)', async () => {
  const { server, api } = setup();
  server.publicLinksAllowed = false;
  expect((await api.getMe()).publicLinksAllowed).toBe(false);
  server.publicLinksAllowed = null;
  expect((await api.getMe()).publicLinksAllowed).toBeNull();
});

it('reports a deletion-pending account', async () => {
  const { server, api } = setup();
  server.status = 'deletion_pending';
  expect(await api.getMe()).toEqual({
    step: 'done',
    deletionPending: true,
    publicLinksAllowed: true,
  });
});

it('maps the age gate codes', async () => {
  const { server, api } = setup();
  expect(await kindOf(api.submitBirthDate('2020-01-01'))).toBe('refused');
  expect(server.birthDate).toBeNull();

  const second = setup();
  await second.api.submitBirthDate('2000-01-01');
  expect(await kindOf(second.api.submitBirthDate('1990-01-01'))).toBe('already_submitted');
  expect(second.server.birthDate).toBe('2000-01-01');
});

it('maps profile and handle failures', async () => {
  const { server, api } = setup();
  await api.submitBirthDate('2000-01-01');
  expect(await kindOf(api.saveDisplayName('badword'))).toBe('invalid');
  expect(await kindOf(api.claimHandle('taken'))).toBe('taken');
  server.overrides.set('/v1/me/handle', problem(429, 'HANDLE_CHANGE_COOLDOWN'));
  expect(await kindOf(api.claimHandle('mai'))).toBe('rate_limited');
  server.overrides.set('/v1/me/profile', problem(503, 'TEMPORARILY_UNAVAILABLE'));
  expect(await kindOf(api.saveDisplayName('Mai'))).toBe('unavailable');
  server.overrides.set('/v1/me/profile', problem(503, 'SERVER_BUSY'));
  expect(await kindOf(api.saveDisplayName('Mai'))).toBe('busy');
  await api.saveDisplayName('Mai');
  await api.claimHandle('mai');
  expect(await api.getMe()).toEqual({
    step: 'done',
    deletionPending: false,
    publicLinksAllowed: true,
  });
  const put = server.auth.requests.find((r) => r.method === 'PUT' && r.path === '/v1/me/handle');
  expect(put?.body).toEqual({ handle: 'taken' });
});

it('rejects a malformed /v1/me body instead of guessing a step', async () => {
  const { server, api } = setup();
  server.overrides.set('/v1/me', json({ onboarding: { step: 'admin' } }));
  expect(await kindOf(api.getMe())).toBe('unavailable');
});
