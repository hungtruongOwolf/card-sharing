import { problem } from '../../../test/fakeAuthServer';
import { FakeSocialServer, ME, SHARE_HOST, tokenFor } from '../../../test/fakeSocialServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import {
  createFriendsApi,
  FriendsError,
  inviteTokenFromUrl,
  parseInvite,
  parseRequestList,
  parseSearch,
  parseSettings,
} from '../friendsApi';
import { newIdempotencyKey } from '../idempotencyKey';

function setup() {
  const server = new FakeSocialServer();
  const store = memoryStore(sessionFromTokenPair(server.auth.issue()));
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store,
    fetch: server.fetch,
    now: server.now,
  });
  return { server, api: createFriendsApi(client.fetch) };
}

const lastRequest = (server: FakeSocialServer) => server.auth.requests.at(-1);

describe('invites', () => {
  it('reads and resets my invite link', async () => {
    const { server, api } = setup();
    const first = await api.myInvite();
    expect(first).toEqual({ url: `${SHARE_HOST}/i/${tokenFor('mine')}`, expiresAt: null });
    const next = await api.resetInvite();
    expect(next.url).not.toBe(first.url);
    await expect(api.previewInvite(tokenFor('mine'))).rejects.toMatchObject({
      kind: 'invite_expired',
    });
    expect(server.count('POST', '/v1/me/invite/reset')).toBe(1);
  });

  it('carries a minor invite expiry', async () => {
    const { server, api } = setup();
    server.myInviteExpiresAt = '2026-11-12T10:00:00Z';
    expect((await api.myInvite()).expiresAt).toBe(Date.parse('2026-11-12T10:00:00Z'));
  });

  it('previews an invite and says when it is my own', async () => {
    const { server, api } = setup();
    const mai = server.addUser('Mai');
    server.invites.set(tokenFor('mai'), mai.id);
    expect(await api.previewInvite(tokenFor('mai'))).toEqual({ displayName: 'Mai', isSelf: false });
    expect(await api.previewInvite(tokenFor('mine'))).toMatchObject({ isSelf: true });
    await expect(api.previewInvite(tokenFor('nobody'))).rejects.toMatchObject({
      kind: 'not_found',
    });
  });

  it('never puts a malformed token into the path', async () => {
    const { server, api } = setup();
    const before = server.auth.requests.length;
    await expect(api.previewInvite('../me/invite/reset')).rejects.toMatchObject({
      kind: 'not_found',
    });
    expect(server.auth.requests.length).toBe(before);
  });

  it('extracts the token from an invite url only', () => {
    expect(inviteTokenFromUrl(`${SHARE_HOST}/i/${tokenFor('a')}`)).toBe(tokenFor('a'));
    expect(inviteTokenFromUrl(`${SHARE_HOST}/i/${tokenFor('a')}?x=1`)).toBeNull();
    expect(inviteTokenFromUrl(`javascript:alert(1)//i/${tokenFor('a')}`)).toBeNull();
    expect(parseInvite({ url: 'https://evil.test/x' })).toBeNull();
    expect(parseInvite({ url: `${SHARE_HOST}/i/${tokenFor('a')}`, expires_at: 'soon' })).toBeNull();
  });
});

describe('friend requests', () => {
  it('sends a request from an invite with the idempotency key and replays a retry', async () => {
    const { server, api } = setup();
    const mai = server.addUser('Mai');
    server.invites.set(tokenFor('mai'), mai.id);
    const key = newIdempotencyKey();
    await api.sendRequest({ inviteToken: tokenFor('mai') }, key);
    await api.sendRequest({ inviteToken: tokenFor('mai') }, key);
    expect(lastRequest(server)).toMatchObject({
      method: 'POST',
      path: '/v1/friend-requests',
      idempotencyKey: key,
      body: { invite_token: tokenFor('mai') },
    });
    expect(await api.listRequests('outgoing')).toHaveLength(1);
  });

  it('refuses my own invite and maps caps to typed errors', async () => {
    const { server, api } = setup();
    await expect(
      api.sendRequest({ inviteToken: tokenFor('mine') }, newIdempotencyKey()),
    ).rejects.toMatchObject({ kind: 'invalid' });
    const mai = server.addUser('Mai');
    server.invites.set(tokenFor('mai'), mai.id);
    server.overrides.set('POST /v1/friend-requests', problem(409, 'FRIEND_LIMIT_REACHED'));
    await expect(
      api.sendRequest({ inviteToken: tokenFor('mai') }, newIdempotencyKey()),
    ).rejects.toMatchObject({ kind: 'friend_limit' });
    server.overrides.set('POST /v1/friend-requests', problem(429, 'RATE_LIMITED'));
    await expect(
      api.sendRequest({ inviteToken: tokenFor('mai') }, newIdempotencyKey()),
    ).rejects.toMatchObject({ kind: 'rate_limited' });
  });

  it('looks the same when the recipient blocked me', async () => {
    const { server, api } = setup();
    const ben = server.addUser('Ben', { handle: 'ben.dog' });
    const found = await api.searchHandle('@Ben.Dog');
    if (!found) throw new Error('expected a result');
    server.blocks.set(ben.id, server.now()); // blocks after the search: the send is silently dropped
    await expect(api.sendRequest({ searchRef: found.ref }, newIdempotencyKey())).resolves.toBe(
      undefined,
    );
  });

  it('lists incoming requests with the minor hint and answers them', async () => {
    const { server, api } = setup();
    const ana = server.addUser('Ana', { handle: 'ana', adultNoMutualFriends: true });
    const bo = server.addUser('Bo');
    const fromAna = server.addRequest(ana, 'incoming');
    const fromBo = server.addRequest(bo, 'incoming');
    const incoming = await api.listRequests('incoming');
    expect(incoming.map((r) => [r.person.displayName, r.adultNoMutualFriends])).toEqual([
      ['Ana', true],
      ['Bo', false],
    ]);
    expect(await api.accept(fromAna.id)).toEqual({ id: ana.id, displayName: 'Ana', handle: 'ana' });
    await api.decline(fromBo.id);
    expect(await api.listRequests('incoming')).toEqual([]);
    await expect(api.accept(fromBo.id)).rejects.toMatchObject({ kind: 'not_found' });
  });

  it('cancels an outgoing request', async () => {
    const { server, api } = setup();
    const sent = server.addRequest(server.addUser('Cy'), 'outgoing');
    await api.cancel(sent.id);
    expect(lastRequest(server)).toMatchObject({
      method: 'DELETE',
      path: `/v1/friend-requests/${sent.id}`,
    });
    expect(await api.listRequests('outgoing')).toEqual([]);
  });

  it('drops malformed rows and keeps the hint only when the server sent it', () => {
    const row = {
      id: '10000000-0000-4000-8000-000000000001',
      user: { id: '00000000-0000-4000-8000-000000000001', display_name: ' Mai ', handle: 'MAI!' },
      created_at: '2026-11-05T10:00:00Z',
      expires_at: '2026-11-19T10:00:00Z',
    };
    expect(
      parseRequestList({
        items: [row, { ...row, id: 'x' }, { ...row, user: { ...row.user, display_name: '' } }],
      }),
    ).toEqual([
      {
        id: row.id,
        person: { id: row.user.id, displayName: 'Mai', handle: null },
        createdAt: Date.parse(row.created_at),
        expiresAt: Date.parse(row.expires_at),
        adultNoMutualFriends: false,
      },
    ]);
    expect(parseRequestList({ items: [{ ...row, hint: 'something_else' }] })?.[0]).toMatchObject({
      adultNoMutualFriends: false,
    });
    expect(parseRequestList({ items: 'nope' })).toBeNull();
  });
});

describe('friends and blocks', () => {
  it('lists friends, removes one silently and maps a stranger to not_found', async () => {
    const { server, api } = setup();
    const mai = server.addUser('Mai');
    server.friends.add(mai.id);
    expect(await api.listFriends()).toEqual([{ id: mai.id, displayName: 'Mai', handle: null }]);
    await api.removeFriend(mai.id);
    expect(await api.listFriends()).toEqual([]);
    await expect(api.removeFriend(mai.id)).rejects.toMatchObject({ kind: 'not_found' });
  });

  it('blocks a friend, lists and unblocks without restoring the friendship', async () => {
    const { server, api } = setup();
    const mai = server.addUser('Mai');
    server.friends.add(mai.id);
    await api.block(mai.id);
    expect(lastRequest(server)).toMatchObject({ body: { user_id: mai.id } });
    expect(await api.listFriends()).toEqual([]);
    expect((await api.listBlocks()).map((b) => b.person.id)).toEqual([mai.id]);
    await api.unblock(mai.id);
    expect(await api.listBlocks()).toEqual([]);
    expect(await api.listFriends()).toEqual([]);
  });

  it('never sends a non-uuid id', async () => {
    const { server, api } = setup();
    const before = server.auth.requests.length;
    for (const call of [
      () => api.removeFriend('../me'),
      () => api.block(ME.slice(1)),
      () => api.unblock('x'),
      () => api.accept('1/accept'),
    ]) {
      await expect(call()).rejects.toBeInstanceOf(FriendsError);
    }
    expect(server.auth.requests.length).toBe(before);
  });
});

describe('privacy and search', () => {
  it('reads and changes findable, and maps the minor refusal', async () => {
    const { server, api } = setup();
    expect(await api.privacySettings()).toEqual({ findableByHandle: true, canBeFindable: true });
    expect(await api.setFindable(false)).toEqual({ findableByHandle: false, canBeFindable: true });
    server.canBeFindable = false;
    await expect(api.setFindable(true)).rejects.toMatchObject({ kind: 'not_allowed' });
  });

  it('never shows findable on an account that cannot be', () => {
    expect(parseSettings({ findable_by_handle: true, can_be_findable: false })).toEqual({
      findableByHandle: false,
      canBeFindable: false,
    });
    expect(parseSettings({ findable_by_handle: 'yes', can_be_findable: true })).toBeNull();
  });

  it('answers null for a missing, hidden or own handle', async () => {
    const { server, api } = setup();
    server.addUser('Hidden', { handle: 'hidden', findable: false });
    expect(await api.searchHandle('nobody')).toBeNull();
    expect(await api.searchHandle('hidden')).toBeNull();
    expect(await api.searchHandle(server.myHandle)).toBeNull();
  });

  it('rejects a search body that is not a search response', () => {
    expect(parseSearch({ result: null })).toBeNull();
    expect(parseSearch({})).toBeUndefined();
    expect(parseSearch({ result: { ref: 'a b', display_name: 'Mai' } })).toBeUndefined();
    expect(parseSearch({ result: { ref: 'abc', display_name: 'Mai' } })).toEqual({
      ref: 'abc',
      displayName: 'Mai',
    });
  });
});

describe('idempotency key', () => {
  it('is a fresh version-4 uuid, also without crypto.randomUUID', () => {
    const v4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
    const original = globalThis.crypto;
    try {
      Object.defineProperty(globalThis, 'crypto', { value: undefined, configurable: true });
      const a = newIdempotencyKey();
      expect(a).toMatch(v4);
      expect(newIdempotencyKey()).not.toBe(a);
    } finally {
      Object.defineProperty(globalThis, 'crypto', { value: original, configurable: true });
    }
  });
});
