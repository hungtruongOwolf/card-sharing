import { useQueryClient } from '@tanstack/react-query';
import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useSyncExternalStore,
  type ReactNode,
} from 'react';

import type { AuthClient, AuthState } from './authClient';

const AuthContext = createContext<AuthClient | null>(null);

interface Props {
  client: AuthClient;
  children: ReactNode;
  /** Also runs on every sign-out: clears device copies of server data (e.g. the offline feed). */
  onSignedOut?: () => void;
}

/**
 * Provides the auth client and wires sign-out to the query cache: whenever the device leaves the signed-in
 * state (logout, revoked session, reuse detection), every cached server response is dropped so the next
 * account cannot see the previous one's data. Must sit inside `QueryClientProvider`.
 */
export function AuthProvider({ client, children, onSignedOut }: Props) {
  const queryClient = useQueryClient();
  const signedOut = useRef(onSignedOut);
  useEffect(() => {
    signedOut.current = onSignedOut;
  }, [onSignedOut]);

  useEffect(() => {
    const unsubscribe = client.subscribe((state) => {
      if (state.status !== 'signedOut') return;
      queryClient.clear();
      signedOut.current?.();
    });
    void client.init();
    return unsubscribe;
  }, [client, queryClient]);

  return <AuthContext.Provider value={client}>{children}</AuthContext.Provider>;
}

export function useAuthClient(): AuthClient {
  const client = useContext(AuthContext);
  if (!client) throw new Error('useAuthClient must be used inside AuthProvider');
  return client;
}

/** Current auth state; re-renders on sign-in and sign-out. */
export function useAuthState(): AuthState {
  const client = useAuthClient();
  return useSyncExternalStore(client.subscribe, client.getState);
}
