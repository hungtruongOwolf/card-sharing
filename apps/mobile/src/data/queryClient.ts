import { QueryClient } from '@tanstack/react-query';

/**
 * A 429 from any of the app's API clients: their errors carry the HTTP `status` (and some a `rate_limited` kind).
 * Retrying one right away only spends more of the same limit (B7).
 */
export function isRateLimited(error: unknown): boolean {
  if (typeof error !== 'object' || error === null) return false;
  const { status, kind } = error as { status?: unknown; kind?: unknown };
  return status === 429 || kind === 'rate_limited';
}

/** One retry for a failed query, never for a rate-limited one. */
export const retryQuery = (failureCount: number, error: unknown): boolean =>
  failureCount < 1 && !isRateLimited(error);

/**
 * Server-state cache (TanStack Query, conventions.md). In memory only: it is never persisted, so no
 * response data or credential outlives the process. It is cleared on every sign-out (AuthProvider).
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: retryQuery, staleTime: 30_000 },
      mutations: { retry: 0 },
    },
  });
}
