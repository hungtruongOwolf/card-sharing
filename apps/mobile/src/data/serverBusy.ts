/** Problem code of a 503 the server sends when it is overloaded rather than down (api-mvp "Rate limits"). */
export const SERVER_BUSY = 'SERVER_BUSY';

/**
 * Whether an error is the server saying "busy, try again shortly" (503 `SERVER_BUSY`, with `Retry-After`), as opposed
 * to the server being unreachable. Works on any of the app's API error classes that carry `status` and `code`, or a
 * `busy` kind.
 */
export function isServerBusy(error: unknown): boolean {
  if (typeof error !== 'object' || error === null) return false;
  const { status, code, kind } = error as { status?: unknown; code?: unknown; kind?: unknown };
  return status === 503 && (code === SERVER_BUSY || kind === 'busy');
}
