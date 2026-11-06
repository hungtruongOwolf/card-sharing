/** Report choices and outcomes shared by the server route and the browser form (no server imports). */

/** AC-26.1 reasons, as the API stores them (`reports.reason`). */
export const REPORT_REASONS = [
  { value: 'sexual', label: 'Nudity or sexual content' },
  { value: 'minor_safety', label: 'A minor may be at risk' },
  { value: 'harassment', label: 'Harassment or bullying' },
  { value: 'violence', label: 'Violence or gore' },
  { value: 'hate', label: 'Hate' },
  { value: 'spam', label: 'Spam' },
  { value: 'other', label: 'Something else' },
] as const;

export type ReportReason = (typeof REPORT_REASONS)[number]['value'];

/** `reports.note` bound. */
export const MAX_REPORT_NOTE = 500;

/** Outcome shown to the guest; never says more about the link than the page already does. */
export type ReportOutcome = 'accepted' | 'invalid' | 'captcha' | 'gone' | 'limited' | 'unavailable';
