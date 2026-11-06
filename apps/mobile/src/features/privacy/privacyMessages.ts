import { getLocale, t } from '../../i18n';
import { AuthError } from '../auth';
import { PrivacyError } from './privacyApi';

/** The device is already signed out (the auth client cleared it); the stack moves to sign-in by itself. */
export const sessionEnded = (error: unknown): boolean =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

/** A date and time in the device locale, e.g. "Nov 13, 2026, 10:00 AM"; ISO when `Intl` is missing. */
export function formatDateTime(epochMs: number, locale: string = getLocale()): string {
  try {
    return new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }).format(
      new Date(epochMs),
    );
  } catch {
    return new Date(epochMs).toISOString().slice(0, 16).replace('T', ' ');
  }
}

/** One message per failure; the copy says no more than the server told us. */
export function privacyErrorMessage(error: unknown): string {
  if (error instanceof AuthError && error.status === 429) return t('privacyData.error.rateLimited');
  if (!(error instanceof PrivacyError)) return t('privacyData.error.generic');
  switch (error.kind) {
    case 'reauth_required':
      return t('privacyData.error.reauthExpired');
    case 'rate_limited':
      return t('privacyData.error.rateLimited');
    default:
      return t('privacyData.error.generic');
  }
}
