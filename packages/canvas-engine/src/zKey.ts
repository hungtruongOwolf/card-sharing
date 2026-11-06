/**
 * Fractional-index z keys for board pins (ADR-0007): reordering one pin writes one key and never renumbers the
 * others. Keys are base-62 strings (`[0-9A-Za-z]`, compared by code unit) made of an integer part, whose head
 * letter encodes its length (`a0`, `a1` ... `b10` ...), and an optional fraction without trailing zeros. Appending
 * on top grows the key logarithmically, so "bring to front" stays short. The algorithm is the public-domain
 * "fractional indexing" scheme by David Greenspan (CC0), also used by the backend's property tests (T-110).
 */

export const Z_DIGITS = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';
/** The server refuses longer keys (api-mvp validation). */
export const MAX_Z_KEY_LENGTH = 32;

const ZERO = Z_DIGITS[0]!;
const LAST = Z_DIGITS[Z_DIGITS.length - 1]!;
const SMALLEST_INTEGER = `A${ZERO.repeat(26)}`;

export class ZKeyError extends Error {}

function midpoint(a: string, b: string | null): string {
  if (b !== null && a >= b) throw new ZKeyError('keys out of order');
  if (a.endsWith(ZERO) || (b !== null && b.endsWith(ZERO))) throw new ZKeyError('trailing zero');
  if (b !== null) {
    let n = 0;
    while ((a[n] ?? ZERO) === b[n]) n++;
    if (n > 0) return b.slice(0, n) + midpoint(a.slice(n), b.slice(n));
  }
  const digitA = a ? Z_DIGITS.indexOf(a[0]!) : 0;
  const digitB = b !== null ? Z_DIGITS.indexOf(b[0]!) : Z_DIGITS.length;
  if (digitB - digitA > 1) return Z_DIGITS[Math.round((digitA + digitB) / 2)]!;
  if (b !== null && b.length > 1) return b.slice(0, 1);
  return Z_DIGITS[digitA]! + midpoint(a.slice(1), null);
}

function integerLength(head: string): number {
  if (head >= 'a' && head <= 'z') return head.charCodeAt(0) - 'a'.charCodeAt(0) + 2;
  if (head >= 'A' && head <= 'Z') return 'Z'.charCodeAt(0) - head.charCodeAt(0) + 2;
  throw new ZKeyError('invalid head');
}

function integerPart(key: string): string {
  const length = integerLength(key[0] ?? '');
  if (length > key.length) throw new ZKeyError('integer part too short');
  return key.slice(0, length);
}

/** True when `key` is a well-formed z key the server accepts. */
export function isZKey(key: string): boolean {
  if (key.length === 0 || key.length > MAX_Z_KEY_LENGTH) return false;
  if ([...key].some((c) => !Z_DIGITS.includes(c))) return false;
  try {
    if (key === SMALLEST_INTEGER) return false;
    const fraction = key.slice(integerPart(key).length);
    return !fraction.endsWith(ZERO);
  } catch {
    return false;
  }
}

function validate(key: string): void {
  if (!isZKey(key)) throw new ZKeyError('invalid key');
}

function incrementInteger(x: string): string | null {
  const head = x[0]!;
  const digits = x.slice(1).split('');
  let carry = true;
  for (let i = digits.length - 1; carry && i >= 0; i--) {
    const d = Z_DIGITS.indexOf(digits[i]!) + 1;
    if (d === Z_DIGITS.length) digits[i] = ZERO;
    else {
      digits[i] = Z_DIGITS[d]!;
      carry = false;
    }
  }
  if (!carry) return head + digits.join('');
  if (head === 'Z') return `a${ZERO}`;
  if (head === 'z') return null;
  const next = String.fromCharCode(head.charCodeAt(0) + 1);
  if (next > 'a') digits.push(ZERO);
  else digits.pop();
  return next + digits.join('');
}

function decrementInteger(x: string): string | null {
  const head = x[0]!;
  const digits = x.slice(1).split('');
  let borrow = true;
  for (let i = digits.length - 1; borrow && i >= 0; i--) {
    const d = Z_DIGITS.indexOf(digits[i]!) - 1;
    if (d === -1) digits[i] = LAST;
    else {
      digits[i] = Z_DIGITS[d]!;
      borrow = false;
    }
  }
  if (!borrow) return head + digits.join('');
  if (head === 'a') return `Z${LAST}`;
  if (head === 'A') return null;
  const next = String.fromCharCode(head.charCodeAt(0) - 1);
  if (next < 'Z') digits.push(LAST);
  else digits.pop();
  return next + digits.join('');
}

/**
 * A key strictly between `a` and `b` (null = open end). Throws `ZKeyError` on invalid or unordered input; the
 * caller checks `MAX_Z_KEY_LENGTH` on the result (only repeated inserts into one gap can grow it that far).
 */
export function zKeyBetween(a: string | null, b: string | null): string {
  if (a !== null) validate(a);
  if (b !== null) validate(b);
  if (a !== null && b !== null && a >= b) throw new ZKeyError('keys out of order');
  if (a === null) {
    if (b === null) return `a${ZERO}`;
    const ib = integerPart(b);
    const fb = b.slice(ib.length);
    if (ib === SMALLEST_INTEGER) return ib + midpoint('', fb);
    if (ib < b) return ib;
    const lower = decrementInteger(ib);
    if (lower === null) throw new ZKeyError('cannot go lower');
    return lower;
  }
  const ia = integerPart(a);
  const fa = a.slice(ia.length);
  if (b === null) {
    const higher = incrementInteger(ia);
    return higher === null ? ia + midpoint(fa, null) : higher;
  }
  const ib = integerPart(b);
  const fb = b.slice(ib.length);
  if (ia === ib) return ia + midpoint(fa, fb);
  const higher = incrementInteger(ia);
  if (higher === null) throw new ZKeyError('cannot go higher');
  return higher < b ? higher : ia + midpoint(fa, null);
}

/** Key for a pin brought to the front of `keys` (any order). */
export function zKeyOnTop(keys: readonly string[]): string {
  const top = keys.reduce<string | null>((max, k) => (max === null || k > max ? k : max), null);
  return zKeyBetween(top, null);
}

/** Key for a pin sent to the back of `keys` (any order). */
export function zKeyAtBottom(keys: readonly string[]): string {
  const bottom = keys.reduce<string | null>((min, k) => (min === null || k < min ? k : min), null);
  return zKeyBetween(null, bottom);
}
