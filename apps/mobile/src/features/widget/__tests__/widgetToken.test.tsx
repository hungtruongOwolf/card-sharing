import { render, waitFor } from '@testing-library/react-native';
import { act } from 'react';
import type { AppStateStatus } from 'react-native';

import { FakeAuthServer, json, problem } from '../../../test/fakeAuthServer';
import { memoryStore } from '../../../test/onboardingHarness';
import { secureStoreMock as secureStore } from '../../../test/secureStoreMock';
import { createAuthClient } from '../../auth/authClient';
import { sessionFromTokenPair } from '../../auth/session';
import { createWidgetTokenApi, parseWidgetToken, type WidgetToken } from '../widgetToken';
import {
  createSecureWidgetTokenStore,
  WIDGET_TOKEN_KEY,
  type WidgetTokenStore,
} from '../widgetTokenStore';
import { ensureWidgetToken, hasHomeScreenWidget, WidgetTokenSync } from '../WidgetTokenSync';

// eslint-disable-next-line @typescript-eslint/no-require-imports -- jest.mock factories must use require()
jest.mock('expo-secure-store', () => require('../../../test/secureStoreMock').secureStoreMock);

const DAY = 24 * 3_600_000;
const PATH = '/v1/me/widget-token';

/** The auth fake plus `POST /v1/me/widget-token` as apps/api/src/routes/widget.rs answers it (201, 30-day token). */
function setup() {
  const server = new FakeAuthServer();
  let minted = 0;
  const failNext: Response[] = [];
  server.routes = (request) => {
    if (request.method !== 'POST' || request.path !== PATH) return null;
    const fail = failNext.shift();
    if (fail) return fail;
    minted += 1;
    return json(
      {
        token: `wid_${String(minted).padStart(20, '0')}`,
        expires_at: new Date(server.nowMs + 30 * DAY).toISOString(),
      },
      201,
    );
  };
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: memoryStore(sessionFromTokenPair(server.issue())),
    fetch: server.fetch,
    now: server.now,
  });
  return {
    server,
    failNext,
    minted: () => minted,
    api: createWidgetTokenApi(client.fetch),
  };
}

function memoryTokenStore(initial: WidgetToken | null = null): WidgetTokenStore & {
  current: WidgetToken | null;
} {
  const store = {
    current: initial,
    load: async () => store.current,
    save: async (token: WidgetToken) => {
      store.current = token;
    },
    clear: async () => {
      store.current = null;
    },
  };
  return store;
}

function fakeAppState() {
  const listeners: ((state: AppStateStatus) => void)[] = [];
  return {
    currentState: 'active' as AppStateStatus,
    addEventListener: (_: 'change', listener: (state: AppStateStatus) => void) => {
      listeners.push(listener);
      return { remove: () => listeners.splice(listeners.indexOf(listener), 1) };
    },
    emit(state: AppStateStatus) {
      for (const listener of [...listeners]) listener(state);
    },
  };
}

describe('widget token api', () => {
  it('mints a token through the session', async () => {
    const { server, api } = setup();
    const token = await api.mint();
    expect(token.token).toMatch(/^wid_/);
    expect(token.expiresAt).toBe(server.nowMs + 30 * DAY);
    expect(server.requests.at(-1)).toMatchObject({ method: 'POST', path: PATH });
    expect(server.requests.at(-1)?.authorization).toMatch(/^Bearer /);
  });

  it('maps 403 and 429 and refuses a malformed body', async () => {
    const { api, failNext } = setup();
    failNext.push(problem(403, 'FORBIDDEN'));
    await expect(api.mint()).rejects.toMatchObject({ kind: 'not_allowed' });
    failNext.push(problem(429, 'RATE_LIMITED'));
    await expect(api.mint()).rejects.toMatchObject({ kind: 'rate_limited' });
    failNext.push(json({ token: 'short', expires_at: 'x' }, 201));
    await expect(api.mint()).rejects.toMatchObject({ kind: 'unavailable' });
    expect(
      parseWidgetToken({ token: 'has space in it 1234', expires_at: '2026-12-01T00:00:00Z' }),
    ).toBeNull();
  });
});

describe('secure widget token store', () => {
  beforeEach(() => secureStore.__reset());

  it('round-trips, clears, and is readable after first unlock on this device only', async () => {
    const store = createSecureWidgetTokenStore();
    const token = {
      token: 'wid_00000000000000000001',
      expiresAt: Date.parse('2026-12-01T00:00:00Z'),
    };
    await store.save(token);
    await expect(store.load()).resolves.toEqual(token);
    await store.clear();
    await expect(store.load()).resolves.toBeNull();
    expect(secureStore.__calls.every((c) => c.key === WIDGET_TOKEN_KEY)).toBe(true);
    expect(
      secureStore.__calls.every(
        (c) =>
          (c.options as { keychainAccessible?: number }).keychainAccessible ===
          secureStore.AFTER_FIRST_UNLOCK_THIS_DEVICE_ONLY,
      ),
    ).toBe(true);
  });

  it('treats a corrupt entry as no token', async () => {
    secureStore.__items.set(WIDGET_TOKEN_KEY, '{not json');
    await expect(createSecureWidgetTokenStore().load()).resolves.toBeNull();
  });
});

describe('ensureWidgetToken', () => {
  it('mints when nothing is stored, then keeps the stored token', async () => {
    const { server, api, minted } = setup();
    const store = memoryTokenStore();
    await expect(ensureWidgetToken(api, store, server.nowMs)).resolves.toBe(true);
    expect(store.current?.token).toMatch(/^wid_/);
    await expect(ensureWidgetToken(api, store, server.nowMs)).resolves.toBe(false);
    expect(minted()).toBe(1);
  });

  it('replaces an expired token', async () => {
    const { server, api, minted } = setup();
    const store = memoryTokenStore({
      token: 'wid_old_000000000000000',
      expiresAt: server.nowMs,
    });
    await ensureWidgetToken(api, store, server.nowMs);
    expect(minted()).toBe(1);
    expect(store.current?.token).not.toBe('wid_old_000000000000000');
  });

  it('drops a token minted after the device signed out', async () => {
    const { server, api } = setup();
    const store = memoryTokenStore();
    await expect(ensureWidgetToken(api, store, server.nowMs, () => false)).resolves.toBe(false);
    expect(store.current).toBeNull();
  });
});

describe('WidgetTokenSync', () => {
  it('mints on launch and retries on the next foreground after a failure', async () => {
    const { server, api, failNext, minted } = setup();
    failNext.push(problem(503, 'TEMPORARILY_UNAVAILABLE'));
    const store = memoryTokenStore();
    const appState = fakeAppState();
    await render(
      <WidgetTokenSync
        api={api}
        store={store}
        now={server.now}
        appState={appState as unknown as Parameters<typeof WidgetTokenSync>[0]['appState']}
      />,
    );
    await waitFor(() => expect(server.count('POST', PATH)).toBe(1));
    expect(store.current).toBeNull();
    await act(async () => {
      appState.emit('background');
      appState.emit('active');
      await Promise.resolve();
    });
    await waitFor(() => expect(store.current).not.toBeNull());
    expect(minted()).toBe(1);
  });

  it('mints nothing on the web, which has no widget (W1)', async () => {
    const { server, api } = setup();
    const store = memoryTokenStore();
    const appState = fakeAppState();
    await render(
      <WidgetTokenSync
        api={api}
        store={store}
        now={server.now}
        os="web"
        appState={appState as unknown as Parameters<typeof WidgetTokenSync>[0]['appState']}
      />,
    );
    await act(async () => {
      appState.emit('background');
      appState.emit('active');
      await Promise.resolve();
    });
    expect(server.count('POST', PATH)).toBe(0);
    expect(store.current).toBeNull();
    expect(hasHomeScreenWidget('web')).toBe(false);
    expect(hasHomeScreenWidget('ios')).toBe(true);
    expect(hasHomeScreenWidget('android')).toBe(true);
  });
});
