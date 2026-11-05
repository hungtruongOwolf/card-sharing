/**
 * A fresh UUIDv4 for an `Idempotency-Key` header (the server requires a UUID). The key is not a secret: the
 * server scopes it to the caller and the route, so it only has to be unique per attempt. `crypto.randomUUID`
 * is used where the runtime has it; Hermes may not, so the fallback builds a version-4 UUID from
 * `Math.random`, which is enough for uniqueness and needs no extra native module.
 *
 * Make one key per logical action and reuse it for retries of that action, so a lost response cannot
 * create a second request.
 */
export function newIdempotencyKey(): string {
  const native = (globalThis as { crypto?: { randomUUID?: () => string } }).crypto?.randomUUID;
  if (typeof native === 'function') return native.call((globalThis as { crypto: object }).crypto);
  const hex = Array.from({ length: 32 }, () => Math.floor(Math.random() * 16));
  hex[12] = 4; // version 4
  hex[16] = 8 + ((hex[16] ?? 0) % 4); // RFC 4122 variant (8, 9, a or b)
  const s = hex.map((n) => n.toString(16)).join('');
  return `${s.slice(0, 8)}-${s.slice(8, 12)}-${s.slice(12, 16)}-${s.slice(16, 20)}-${s.slice(20)}`;
}
