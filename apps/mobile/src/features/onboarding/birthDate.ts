/**
 * Date-of-birth parsing for the neutral DOB screen (prd-mvp AC-2.4, threat-model SR-24/SR-25).
 *
 * Deliberately age-blind: this module checks only that the input is a real calendar date between 1900 and
 * today (the same plausibility bounds as `crates/identity/src/age_gate.rs`). It never computes an age and
 * never sees the region's `min_age`; the server alone decides admission. Anything else would leak the
 * threshold to someone probing the form.
 */

export type DatePart = 'day' | 'month' | 'year';

export interface BirthDateInput {
  readonly day: string;
  readonly month: string;
  readonly year: string;
}

export const EMPTY_BIRTH_DATE: BirthDateInput = { day: '', month: '', year: '' };

const EARLIEST_YEAR = 1900;
const DEFAULT_ORDER: readonly DatePart[] = ['month', 'day', 'year'];

/**
 * Field order of the device locale (e.g. month/day/year for en-US, day/month/year for en-GB). Only the order
 * is taken from the sample date; nothing is shown or pre-filled.
 */
export function datePartOrder(locale?: string): readonly DatePart[] {
  try {
    const parts = new Intl.DateTimeFormat(locale, {
      day: 'numeric',
      month: 'numeric',
      year: 'numeric',
    }).formatToParts(new Date(Date.UTC(2000, 10, 22, 12)));
    const order = parts
      .map((p) => p.type)
      .filter((type): type is DatePart => type === 'day' || type === 'month' || type === 'year');
    return order.length === 3 && new Set(order).size === 3 ? order : DEFAULT_ORDER;
  } catch {
    return DEFAULT_ORDER;
  }
}

const pad = (value: number, width: number) => String(value).padStart(width, '0');

/**
 * Returns `YYYY-MM-DD` for a complete, real date not after `today`, else null. `today` is the device's
 * local calendar date; the server re-checks against its own clock.
 */
export function toIsoBirthDate(input: BirthDateInput, today: Date): string | null {
  if (!/^\d{1,2}$/.test(input.day) || !/^\d{1,2}$/.test(input.month)) return null;
  if (!/^\d{4}$/.test(input.year)) return null;
  const day = Number(input.day);
  const month = Number(input.month);
  const year = Number(input.year);
  if (year < EARLIEST_YEAR || month < 1 || month > 12 || day < 1) return null;
  const daysInMonth = new Date(Date.UTC(year, month, 0)).getUTCDate();
  if (day > daysInMonth) return null;
  const iso = `${pad(year, 4)}-${pad(month, 2)}-${pad(day, 2)}`;
  const todayIso = `${pad(today.getFullYear(), 4)}-${pad(today.getMonth() + 1, 2)}-${pad(today.getDate(), 2)}`;
  return iso <= todayIso ? iso : null;
}

/** True once every field has the digits it needs, so an error is not shown while the user is still typing. */
export const isBirthDateComplete = (input: BirthDateInput): boolean =>
  input.day.length > 0 && input.month.length > 0 && input.year.length === 4;
