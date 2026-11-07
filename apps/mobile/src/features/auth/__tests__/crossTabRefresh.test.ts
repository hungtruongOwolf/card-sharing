import { FakeAuthServer } from '../../../test/fakeAuthServer';
import { createAuthClient, type AuthClient, type AuthState } from '../authClient';
import { sessionFromTokenPair, type Session } from '../session';
import {
  createBrowserSessionSync,
  REFRESH_LOCK_NAME,
  type LockManagerLike,
  type StorageChange,
  type StorageEventsLike,
} from '../sessionSync';
import type { TokenStore } from '../tokenStore';

const KEY = 'cs.secure.cs.auth.session.v1';
const TEN_MINUTES = 10 * 60_000;
/** Storage events are delivered as a later task in the other tabs. */
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

/** One browser profile: the shared `localStorage` entry, Web Locks, and `storage` events between its tabs. */
function fakeBrowser(server: FakeAuthServer, { withLocks = true } = {}) {
  let value: Session | null = null;
  const tabs = new Set<(event: StorageChange) => void>();
  let tail: Promise<unknown> = Promise.resolve();
  const lockNames: string[] = [];
  const locks: LockManagerLike = {
    request(name, task) {
      lockNames.push(name);
      const run = tail.then(() => task());
      tail = run.catch(() => undefined);
      return run;
    },
  };

  function openTab({ sync = true } = {}) {
    const listeners = new Set<(event: StorageChange) => void>();
    const deliver = (event: StorageChange) => listeners.forEach((listener) => listener(event));
    tabs.add(deliver);
    const toOtherTabs = (event: StorageChange) =>
      setTimeout(() => tabs.forEach((tab) => tab !== deliver && tab(event)), 0);
    const events: StorageEventsLike = {
      addEventListener: (_type, listener) => void listeners.add(listener),
      removeEventListener: (_type, listener) => void listeners.delete(listener),
    };
    const store: TokenStore = {
      load: async () => value,
      save: async (session) => {
        value = session;
        toOtherTabs({ key: KEY });
      },
      clear: async () => {
        if (value === null) return; // removing a missing key fires no event
        value = null;
        toOtherTabs({ key: KEY });
      },
    };
    const client = createAuthClient({
      baseUrl: () => server.baseUrl,
      store,
      fetch: server.fetch,
      now: server.now,
      sync: sync
        ? createBrowserSessionSync({
            locks: withLocks ? locks : undefined,
            events,
            storageKey: KEY,
          })
        : undefined,
    });
    const states: AuthState[] = [];
    client.subscribe((state) => states.push(state));
    return { client, states };
  }

  return {
    openTab,
    lockNames,
    get stored() {
      return value;
    },
    signIn() {
      value = sessionFromTokenPair(server.issue());
    },
  };
}

async function both(a: AuthClient, b: AuthClient) {
  await Promise.all([a.init(), b.init()]);
}

describe('two tabs of one user (B5)', () => {
  it('control: without coordination the second tab replays a consumed token and revokes the family', async () => {
    const server = new FakeAuthServer();
    const browser = fakeBrowser(server);
    browser.signIn();
    const a = browser.openTab({ sync: false });
    const b = browser.openTab({ sync: false });
    await both(a.client, b.client);
    server.advance(TEN_MINUTES + 1);
    expect((await a.client.fetch('/v1/me')).status).toBe(200);
    server.advance(60_000); // past the 30 s grace window
    await expect(b.client.fetch('/v1/me')).rejects.toMatchObject({ kind: 'session_ended' });
    expect(server.isRevoked()).toBe(true);
  });

  it('adopts the pair another tab rotated to instead of refreshing again', async () => {
    const server = new FakeAuthServer();
    const browser = fakeBrowser(server);
    browser.signIn();
    const a = browser.openTab();
    const b = browser.openTab();
    await both(a.client, b.client);
    server.advance(TEN_MINUTES + 1);
    expect((await a.client.fetch('/v1/me')).status).toBe(200);
    await flush();
    server.advance(60_000);
    expect((await b.client.fetch('/v1/me')).status).toBe(200);
    expect(server.count('POST', '/v1/auth/refresh')).toBe(1);
    expect(server.requests.at(-1)?.authorization).toBe(`Bearer ${browser.stored?.accessToken}`);
    expect(server.isRevoked()).toBe(false);
  });

  it('runs one refresh when both tabs find the token expired at once', async () => {
    const server = new FakeAuthServer();
    const browser = fakeBrowser(server);
    browser.signIn();
    const a = browser.openTab();
    const b = browser.openTab();
    await both(a.client, b.client);
    server.advance(TEN_MINUTES + 1);
    const release = server.holdRefreshes();
    const pending = [a.client.fetch('/v1/me'), b.client.fetch('/v1/me'), a.client.fetch('/v1/me')];
    await flush();
    release();
    expect((await Promise.all(pending)).map((r) => r.status)).toEqual([200, 200, 200]);
    expect(server.count('POST', '/v1/auth/refresh')).toBe(1);
    expect(server.isRevoked()).toBe(false);
    expect(browser.lockNames.every((name) => name === REFRESH_LOCK_NAME)).toBe(true);
  });

  it('re-reads the store before rotating even without the Web Locks API', async () => {
    const server = new FakeAuthServer();
    const browser = fakeBrowser(server, { withLocks: false });
    browser.signIn();
    const a = browser.openTab();
    const b = browser.openTab();
    await both(a.client, b.client);
    server.advance(TEN_MINUTES + 1);
    expect((await a.client.fetch('/v1/me')).status).toBe(200);
    // B asks before the storage event reaches it.
    server.advance(60_000);
    expect((await b.client.fetch('/v1/me')).status).toBe(200);
    expect(server.count('POST', '/v1/auth/refresh')).toBe(1);
    expect(server.isRevoked()).toBe(false);
  });

  it('rotates the adopted pair when it is due as well', async () => {
    const server = new FakeAuthServer();
    const browser = fakeBrowser(server);
    browser.signIn();
    const a = browser.openTab();
    const b = browser.openTab();
    await both(a.client, b.client);
    server.advance(TEN_MINUTES + 1);
    expect((await a.client.fetch('/v1/me')).status).toBe(200);
    // B slept through A's rotation and that pair's whole lifetime.
    server.advance(TEN_MINUTES + 1);
    expect((await b.client.fetch('/v1/me')).status).toBe(200);
    expect(server.count('POST', '/v1/auth/refresh')).toBe(2);
    await flush();
    expect((await a.client.fetch('/v1/me')).status).toBe(200);
    expect(server.count('POST', '/v1/auth/refresh')).toBe(2);
    expect(server.isRevoked()).toBe(false);
  });

  it('follows a sign-out in another tab', async () => {
    const server = new FakeAuthServer();
    const browser = fakeBrowser(server);
    browser.signIn();
    const a = browser.openTab();
    const b = browser.openTab();
    await both(a.client, b.client);
    await a.client.signOut();
    await flush();
    expect(b.client.getState()).toEqual({ status: 'signedOut', signOutReason: 'user' });
    await expect(b.client.fetch('/v1/me')).rejects.toMatchObject({ kind: 'signed_out' });
  });

  it('follows a sign-in in another tab', async () => {
    const server = new FakeAuthServer();
    server.devSeedHandles = new Set(['mai']);
    const browser = fakeBrowser(server);
    const a = browser.openTab();
    const b = browser.openTab();
    await both(a.client, b.client);
    expect(b.client.getState().status).toBe('signedOut');
    await a.client.signInAsSeedUser('mai');
    await flush();
    expect(b.client.getState()).toEqual({ status: 'signedIn' });
    expect((await b.client.fetch('/v1/me')).status).toBe(200);
  });

  it('treats a store another tab cleared during the wait as signed out', async () => {
    const server = new FakeAuthServer();
    const browser = fakeBrowser(server);
    browser.signIn();
    const a = browser.openTab();
    const b = browser.openTab();
    await both(a.client, b.client);
    server.advance(TEN_MINUTES + 1);
    // A signs out; B refreshes before the storage event arrives.
    await a.client.signOut();
    const refreshesBefore = server.count('POST', '/v1/auth/refresh');
    await expect(b.client.fetch('/v1/me')).rejects.toMatchObject({ kind: 'signed_out' });
    expect(b.client.getState()).toEqual({ status: 'signedOut', signOutReason: 'user' });
    expect(server.count('POST', '/v1/auth/refresh')).toBe(refreshesBefore);
  });
});

describe('createBrowserSessionSync', () => {
  it('listens to the session key and to a full clear only', () => {
    const listeners = new Set<(event: StorageChange) => void>();
    const events: StorageEventsLike = {
      addEventListener: (_type, listener) => void listeners.add(listener),
      removeEventListener: (_type, listener) => void listeners.delete(listener),
    };
    const sync = createBrowserSessionSync({ locks: undefined, events, storageKey: KEY });
    const heard = jest.fn();
    const unsubscribe = sync.subscribe(heard);
    const fire = (key: string | null) => listeners.forEach((listener) => listener({ key }));
    fire('cs.secure.cs.feed.cache.v1');
    expect(heard).not.toHaveBeenCalled();
    fire(KEY);
    fire(null);
    expect(heard).toHaveBeenCalledTimes(2);
    unsubscribe();
    fire(KEY);
    expect(heard).toHaveBeenCalledTimes(2);
  });

  it('runs the task directly without the Web Locks API', async () => {
    const sync = createBrowserSessionSync({ locks: undefined, events: undefined, storageKey: KEY });
    await expect(sync.withRefreshLock(async () => 7)).resolves.toBe(7);
    expect(typeof sync.subscribe(() => undefined)).toBe('function');
  });
});
