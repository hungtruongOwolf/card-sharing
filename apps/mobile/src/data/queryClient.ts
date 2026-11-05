import { QueryClient } from '@tanstack/react-query';

/**
 * Server-state cache (TanStack Query, conventions.md). In memory only: it is never persisted, so no
 * response data or credential outlives the process. It is cleared on every sign-out (AuthProvider).
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: 1, staleTime: 30_000 },
      mutations: { retry: 0 },
    },
  });
}
