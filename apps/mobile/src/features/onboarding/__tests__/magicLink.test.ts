import { AuthError } from '../../auth';
import { pkceChallenge } from '../../auth/pkce';
import { FakeAuthServer } from '../../../test/fakeAuthServer';
import { memoryKeyValue, testRandom } from '../../../test/onboardingHarness';
import {
  captureLaunchLink,
  capturedLaunchLink,
  resetLaunchLinkForTests,
  stripLaunchLinkFragment,
} from '../launchLink';
import {
  createMagicLinkSignIn,
  isPlausibleEmail,
  LINK_TTL_MS,
  MAGIC_LINK_INDEX_KEY,
  magicLinkKey,
  MagicLinkStartError,
  parseLinkFragment,
} from '../magicLink';
import { isReturnableHref, PHASE_ROUTES } from '../phaseRoutes';

const RID = '0192f0a1-2b3c-7d4e-8f90-123456789abc';
const TOKEN = 'A'.repeat(43);

function setup(signIn = jest.fn(async () => undefined)) {
  const server = new FakeAuthServer();
  const store = memoryKeyValue();
  const magicLink = createMagicLinkSignIn({
    baseUrl: () => server.baseUrl,
    authClient: { signInWithMagicLink: signIn },
    store,
    fetch: server.fetch,
    now: server.now,
    random: testRandom(),
  });
  return { server, store, magicLink, signIn };
}

describe('parseLinkFragment', () => {
  it('reads the request id and token from the fragment', () => {
    expect(parseLinkFragment(`#rid=${RID}&token=${TOKEN}`)).toEqual({
      requestId: RID,
      token: TOKEN,
    });
    expect(parseLinkFragment(`token=${TOKEN}&rid=${RID}`)).toEqual({
      requestId: RID,
      token: TOKEN,
    });
  });

  it('rejects missing or malformed parts', () => {
    expect(parseLinkFragment('')).toBeNull();
    expect(parseLinkFragment(`#token=${TOKEN}`)).toBeNull();
    expect(parseLinkFragment(`#rid=not-a-uuid&token=${TOKEN}`)).toBeNull();
    expect(parseLinkFragment(`#rid=${RID}&token=short`)).toBeNull();
    expect(parseLinkFragment(`#rid=${RID}&token=${'A'.repeat(42)}%3D`)).toBeNull();
  });
});

describe('isPlausibleEmail', () => {
  it('accepts address-shaped input only', () => {
    expect(isPlausibleEmail('cat@example.com')).toBe(true);
    expect(isPlausibleEmail('cat@example')).toBe(false);
    expect(isPlausibleEmail('cat example.com')).toBe(false);
    expect(isPlausibleEmail('@example.com')).toBe(false);
  });
});

describe('createMagicLinkSignIn', () => {
  it('asks for a link with a PKCE challenge and keeps the verifier under the request id', async () => {
    const { server, store, magicLink } = setup();
    const { requestId } = await magicLink.start('cat@example.com', {
      inviteCode: ' ABCD-EFGH-JKMN ',
      returnTo: '/i/abc',
    });
    const sent = server.linkFor('cat@example.com');
    expect(requestId).toBe(sent.requestId);
    const found = await magicLink.lookup(requestId);
    expect(found.status).toBe('found');
    if (found.status !== 'found') return;
    expect(pkceChallenge(found.link.verifier)).toBe(sent.challenge);
    expect(found.link.inviteCode).toBe('ABCD-EFGH-JKMN');
    expect(found.link.returnTo).toBe('/i/abc');
    // The verifier is never sent with the request.
    expect(JSON.stringify(server.requests.map((r) => r.body))).not.toContain(found.link.verifier);
    expect(JSON.parse(store.items.get(MAGIC_LINK_INDEX_KEY) ?? '[]')).toHaveLength(1);
  });

  it('maps start failures to kinds', async () => {
    const startWith = (
      fetchFn: typeof fetch,
      random = testRandom() as ReturnType<typeof testRandom> | null,
    ) =>
      createMagicLinkSignIn({
        baseUrl: () => 'https://api.test',
        authClient: { signInWithMagicLink: jest.fn() },
        store: memoryKeyValue(),
        fetch: fetchFn,
        random,
      }).start('cat@example.com');
    const answering = (response: Response) => startWith(async () => response);
    await expect(answering(new Response(null, { status: 429 }))).rejects.toEqual(
      new MagicLinkStartError('rate_limited'),
    );
    await expect(answering(new Response(null, { status: 400 }))).rejects.toEqual(
      new MagicLinkStartError('invalid_email'),
    );
    await expect(
      answering(
        new Response(JSON.stringify({ code: 'SERVER_BUSY' }), {
          status: 503,
          headers: { 'Content-Type': 'application/problem+json' },
        }),
      ),
    ).rejects.toEqual(new MagicLinkStartError('busy'));
    await expect(
      answering(new Response(JSON.stringify({ request_id: 'nope' }), { status: 202 })),
    ).rejects.toEqual(new MagicLinkStartError('unavailable'));
    await expect(
      startWith(async () => {
        throw new TypeError('offline');
      }),
    ).rejects.toEqual(new MagicLinkStartError('unavailable'));
    await expect(startWith(new FakeAuthServer().fetch, null)).rejects.toEqual(
      new MagicLinkStartError('not_supported'),
    );
  });

  it('reports a link opened elsewhere as missing and a stale one as expired', async () => {
    const { server, magicLink } = setup();
    expect(await magicLink.lookup(RID)).toEqual({ status: 'missing' });
    expect(await magicLink.lookup('../../etc')).toEqual({ status: 'missing' });
    const { requestId } = await magicLink.start('cat@example.com');
    server.advance(LINK_TTL_MS);
    expect(await magicLink.lookup(requestId)).toEqual({ status: 'expired' });
    // The stale record is gone.
    expect(await magicLink.lookup(requestId)).toEqual({ status: 'missing' });
  });

  it('redeems with the kept verifier and forgets the record', async () => {
    const { server, magicLink, signIn } = setup();
    const { requestId } = await magicLink.start('cat@example.com', {
      inviteCode: 'ABCD-EFGH-JKMN',
    });
    const found = await magicLink.lookup(requestId);
    expect(await magicLink.complete(requestId, server.linkFor('cat@example.com').token)).toBe(
      'signed_in',
    );
    expect(signIn).toHaveBeenCalledWith(
      server.linkFor('cat@example.com').token,
      found.status === 'found' ? found.link.verifier : '',
      { inviteCode: 'ABCD-EFGH-JKMN' },
    );
    expect(await magicLink.lookup(requestId)).toEqual({ status: 'missing' });
  });

  it('keeps the record while the link is still usable and drops it on a final answer', async () => {
    const refusal = (status: number, code: string) => new AuthError('rejected', status, code);
    const answers: unknown[] = [
      refusal(403, 'INVITE_REQUIRED'),
      refusal(403, 'INVITE_INVALID'),
      new AuthError('unavailable', 429, 'RATE_LIMITED'),
      new AuthError('unavailable', 503),
      refusal(410, 'MAGIC_LINK_USED'),
    ];
    const signIn = jest.fn(async () => {
      throw answers.shift();
    });
    const { magicLink } = setup(signIn);
    const { requestId } = await magicLink.start('cat@example.com');
    expect(await magicLink.complete(requestId, TOKEN)).toBe('invite_required');
    expect(await magicLink.complete(requestId, TOKEN, 'WRONG')).toBe('invite_invalid');
    expect(signIn).toHaveBeenLastCalledWith(TOKEN, expect.any(String), { inviteCode: 'WRONG' });
    expect(await magicLink.complete(requestId, TOKEN)).toBe('rate_limited');
    expect(await magicLink.complete(requestId, TOKEN)).toBe('unavailable');
    expect((await magicLink.lookup(requestId)).status).toBe('found');
    expect(await magicLink.complete(requestId, TOKEN)).toBe('used');
    expect(await magicLink.lookup(requestId)).toEqual({ status: 'missing' });
  });

  it('maps the other final answers', async () => {
    for (const [code, outcome] of [
      ['MAGIC_LINK_EXPIRED', 'expired'],
      ['MAGIC_LINK_INVALID', 'invalid'],
      ['ACCOUNT_SUSPENDED', 'suspended'],
    ] as const) {
      const { magicLink } = setup(
        jest.fn(async () => {
          throw new AuthError('rejected', 400, code);
        }),
      );
      const { requestId } = await magicLink.start('cat@example.com');
      expect(await magicLink.complete(requestId, TOKEN)).toBe(outcome);
      expect(await magicLink.lookup(requestId)).toEqual({ status: 'missing' });
    }
    const { magicLink } = setup();
    expect(await magicLink.complete(RID, TOKEN)).toBe('invalid');
  });

  it('keeps at most five records and drops old ones', async () => {
    const { server, store, magicLink } = setup();
    const ids: string[] = [];
    for (let i = 0; i < 7; i++) ids.push((await magicLink.start(`cat${i}@example.com`)).requestId);
    expect(JSON.parse(store.items.get(MAGIC_LINK_INDEX_KEY) ?? '[]')).toHaveLength(5);
    expect(store.items.has(magicLinkKey(ids[0] ?? ''))).toBe(false);
    expect(store.items.has(magicLinkKey(ids[6] ?? ''))).toBe(true);
    // An hour later a new request clears the stale ones.
    server.advance(60 * 60 * 1000);
    await magicLink.start('late@example.com');
    expect(JSON.parse(store.items.get(MAGIC_LINK_INDEX_KEY) ?? '[]')).toHaveLength(1);
    expect([...store.items.keys()].filter((k) => k.startsWith('cs.magiclink.v1.'))).toHaveLength(1);
  });
});

describe('captureLaunchLink', () => {
  afterEach(() => resetLaunchLinkForTests());

  it('reads the fragment on the landing route and removes it from the address bar', () => {
    const history = { state: { router: 1 }, replaceState: jest.fn() };
    captureLaunchLink(
      { pathname: '/auth/link', search: '', hash: `#rid=${RID}&token=${TOKEN}` },
      history,
    );
    expect(capturedLaunchLink()).toEqual({ requestId: RID, token: TOKEN });
    expect(history.replaceState).toHaveBeenCalledWith({ router: 1 }, '', '/auth/link');
  });

  it('strips the fragment again once the landing screen is up', () => {
    const history = { state: 1, replaceState: jest.fn() };
    stripLaunchLinkFragment(
      { pathname: '/auth/link', search: '?a=1', hash: `#token=${TOKEN}` },
      history,
    );
    expect(history.replaceState).toHaveBeenCalledWith(1, '', '/auth/link?a=1');
    stripLaunchLinkFragment({ pathname: '/auth/link', search: '', hash: '' }, history);
    stripLaunchLinkFragment({ pathname: '/', search: '', hash: `#token=${TOKEN}` }, history);
    expect(history.replaceState).toHaveBeenCalledTimes(1);
  });

  it('ignores other routes and malformed fragments', () => {
    const history = { state: null, replaceState: jest.fn() };
    captureLaunchLink(
      { pathname: '/card/x', search: '', hash: `#rid=${RID}&token=${TOKEN}` },
      history,
    );
    captureLaunchLink({ pathname: '/auth/link', search: '', hash: '#token=bad' }, history);
    captureLaunchLink(undefined, history);
    expect(capturedLaunchLink()).toBeNull();
    expect(history.replaceState).not.toHaveBeenCalled();
  });
});

describe('isReturnableHref', () => {
  it('accepts only the app’s own signed-in routes', () => {
    expect(isReturnableHref('/i/abc_DEF-123')).toBe(true);
    expect(isReturnableHref('/card/0192f0a1-2b3c-7d4e-8f90-123456789abc')).toBe(true);
    expect(isReturnableHref('/friends?tab=requests')).toBe(true);
    expect(isReturnableHref('/settings/data-export')).toBe(true);
    expect(isReturnableHref('https://evil.example/i/x')).toBe(false);
    expect(isReturnableHref('//evil.example/i/x')).toBe(false);
    expect(isReturnableHref('/onboarding/birth-date')).toBe(false);
    expect(isReturnableHref('/auth/link')).toBe(false);
    expect(isReturnableHref('/i/a/b')).toBe(false);
    expect(isReturnableHref('/')).toBe(false);
  });

  it('routes the landing screen in the sign-in phase only', () => {
    expect(PHASE_ROUTES.filter(([, name]) => name === 'auth/link')).toEqual([
      ['signIn', 'auth/link'],
    ]);
  });
});
