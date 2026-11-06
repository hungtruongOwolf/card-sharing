/**
 * A UUIDv7 (RFC 9562): 48-bit Unix milliseconds, then random bits. Boards and pins are created offline with
 * client ids (ADR-0007), and time-ordered ids keep the server's index appends cheap. `Math.random` suffices: the
 * id is not a secret, and the server rejects a duplicate id from another actor.
 */
export function uuidv7(now: number = Date.now(), random: () => number = Math.random): string {
  const time = Math.max(0, Math.floor(now)).toString(16).padStart(12, '0').slice(-12);
  const nibbles = Array.from({ length: 20 }, () => Math.floor(random() * 16) & 15);
  nibbles[0] = 7; // version
  nibbles[4] = 8 + (nibbles[4]! % 4); // RFC variant
  const rest = nibbles.map((n) => n.toString(16)).join('');
  return `${time.slice(0, 8)}-${time.slice(8, 12)}-${rest.slice(0, 4)}-${rest.slice(4, 8)}-${rest.slice(8, 20)}`;
}

export const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
