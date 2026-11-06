/** Quiet-hour times move in steps of this many minutes. */
export const QUIET_STEP_MINUTES = 30;
const DAY_MINUTES = 24 * 60;

const pad = (n: number) => String(n).padStart(2, '0');

/** `HH:MM` moved by `steps` × {@link QUIET_STEP_MINUTES}, wrapping around midnight. */
export function shiftClockTime(time: string, steps: number): string {
  const [h = 0, m = 0] = time.split(':').map(Number);
  const moved = h * 60 + m + steps * QUIET_STEP_MINUTES;
  const total = ((moved % DAY_MINUTES) + DAY_MINUTES) % DAY_MINUTES;
  return `${pad(Math.floor(total / 60))}:${pad(total % 60)}`;
}
