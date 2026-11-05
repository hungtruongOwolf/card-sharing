import { t } from '../../i18n';

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
/** Within this window a device counts as active now (the server bumps `last_seen_at` coarsely). */
const ACTIVE_NOW = 2 * MINUTE;

/**
 * "Active now", "5 minutes ago", "3 hours ago", "12 days ago". Relative wording reads the same in every
 * time zone and needs no date formatting in the message catalog (format.ts rejects `date`).
 * A timestamp in the future (clock skew) counts as now.
 */
export function lastSeenLabel(lastSeenAt: number, now: number): string {
  const ago = Math.max(0, now - lastSeenAt);
  if (ago < ACTIVE_NOW) return t('sessions.lastSeen.now');
  if (ago < HOUR) return t('sessions.lastSeen.minutes', { count: Math.floor(ago / MINUTE) });
  if (ago < DAY) return t('sessions.lastSeen.hours', { count: Math.floor(ago / HOUR) });
  return t('sessions.lastSeen.days', { count: Math.floor(ago / DAY) });
}

/**
 * Country name for an ISO code in the active locale, or the code itself where `Intl.DisplayNames` is
 * missing (older Hermes builds).
 */
export function countryName(code: string, locale: string): string {
  try {
    if (typeof Intl !== 'undefined' && typeof Intl.DisplayNames === 'function') {
      return new Intl.DisplayNames([locale], { type: 'region' }).of(code) ?? code;
    }
  } catch {
    // Unknown locale or code: fall back to the code.
  }
  return code;
}
