import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, render } from '@testing-library/react-native';

import { FakeAuthServer } from '../../../test/fakeAuthServer';
import { secureStoreMock } from '../../../test/secureStoreMock';
import { AuthProvider, useAuthState } from '../AuthProvider';
import { createAuthClient } from '../authClient';
import { sessionFromTokenPair, serializeSession } from '../session';
import { createSecureTokenStore, SESSION_KEY } from '../tokenStore';

const NONCE = 'n0nce-0123456789abcdef';

// eslint-disable-next-line @typescript-eslint/no-require-imports -- jest.mock factories must use require()
jest.mock('expo-secure-store', () => require('../../../test/secureStoreMock').secureStoreMock);

let seen: string[] = [];
function Probe() {
  const state = useAuthState();
  seen.push(state.status);
  return null;
}

beforeEach(() => {
  secureStoreMock.__reset();
  seen = [];
});

it('runs the sign-out hook so device copies of server data are cleared', async () => {
  const server = new FakeAuthServer();
  const session = sessionFromTokenPair(server.issue());
  if (!session) throw new Error('bad pair');
  secureStoreMock.__items.set(SESSION_KEY, serializeSession(session));
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: createSecureTokenStore(),
    fetch: server.fetch,
    now: server.now,
  });
  const onSignedOut = jest.fn();
  await render(
    <QueryClientProvider client={new QueryClient()}>
      <AuthProvider client={client} onSignedOut={onSignedOut}>
        <Probe />
      </AuthProvider>
    </QueryClientProvider>,
  );
  await act(() => client.init());
  expect(onSignedOut).not.toHaveBeenCalled();
  await act(() => client.signOut());
  expect(onSignedOut).toHaveBeenCalledTimes(1);
});

it('clears the query cache on logout and on revocation', async () => {
  const server = new FakeAuthServer();
  const session = sessionFromTokenPair(server.issue());
  if (!session) throw new Error('bad pair');
  secureStoreMock.__items.set(SESSION_KEY, serializeSession(session));
  const client = createAuthClient({
    baseUrl: () => server.baseUrl,
    store: createSecureTokenStore(),
    fetch: server.fetch,
    now: server.now,
  });
  const queryClient = new QueryClient();

  await render(
    <QueryClientProvider client={queryClient}>
      <AuthProvider client={client}>
        <Probe />
      </AuthProvider>
    </QueryClientProvider>,
  );
  await act(() => client.init());
  expect(seen.at(-1)).toBe('signedIn');

  queryClient.setQueryData(['me'], { account: { id: 'acc-1' } });
  await act(() => client.signOut());

  expect(queryClient.getQueryData(['me'])).toBeUndefined();
  expect(queryClient.getQueryCache().getAll()).toHaveLength(0);
  expect(seen.at(-1)).toBe('signedOut');
  expect(secureStoreMock.__items.has(SESSION_KEY)).toBe(false);

  // Sign in again, then the server revokes the session (e.g. log out everywhere).
  await act(() => client.signInWithIdToken('apple', 'valid-id-token', NONCE));
  expect(seen.at(-1)).toBe('signedIn');
  queryClient.setQueryData(['me'], { account: { id: 'acc-1' } });
  server.revokeAll();
  await act(async () => {
    await client.fetch('/v1/me').catch(() => undefined);
  });
  expect(queryClient.getQueryCache().getAll()).toHaveLength(0);
  expect(seen.at(-1)).toBe('signedOut');
});
