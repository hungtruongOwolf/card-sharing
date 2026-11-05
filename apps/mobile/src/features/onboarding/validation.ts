/**
 * Client-side checks for the profile step (prd-mvp AC-3.2, AC-3.3; api-mvp "Validation"). They only give fast
 * feedback: the server validates again and runs the profanity filter and the uniqueness check.
 */

import { graphemeCount } from '../../text';

export const DISPLAY_NAME_MIN = 2;
export const DISPLAY_NAME_MAX = 30;
export const HANDLE_MIN = 3;
export const HANDLE_MAX = 20;

const HANDLE_PATTERN = /^[a-z0-9._]{3,20}$/;

/** Grapheme count (what the server counts); shared with captions. */
export { graphemeCount };

export type DisplayNameIssue = 'tooShort' | 'tooLong';

export function displayNameIssue(raw: string): DisplayNameIssue | null {
  const length = graphemeCount(raw.trim());
  if (length < DISPLAY_NAME_MIN) return 'tooShort';
  if (length > DISPLAY_NAME_MAX) return 'tooLong';
  return null;
}

/** Handles are case-insensitive: the field lowercases and drops surrounding spaces as the user types. */
export const normalizeHandle = (raw: string): string => raw.trim().toLowerCase();

export const isValidHandle = (handle: string): boolean => HANDLE_PATTERN.test(handle);
