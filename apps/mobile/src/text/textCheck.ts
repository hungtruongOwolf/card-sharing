import { graphemeCount } from './graphemes';
import { looksBlocked, sanitizeText } from './textFilter';
import type { TextFilterConfig } from './textFilterConfig';

export type TextIssue = 'tooShort' | 'tooLong' | 'notAllowed';

export interface TextCheck {
  /** What will be sent and stored: the sanitized text (bidi controls and invisible characters removed). */
  text: string;
  /** Length in grapheme clusters of the sanitized text, the unit every server limit uses. */
  length: number;
  issue: TextIssue | null;
}

/**
 * Client preview of the server's `TextFilter::check` for a field: sanitize, then length, then the term lists.
 * Without the served `filter` (loading, offline, bad response) it sanitizes and checks length only; the server
 * still refuses blocked text on submit.
 */
export function checkText(
  input: string,
  rules: { min: number; max: number },
  filter?: TextFilterConfig,
): TextCheck {
  const text = filter ? sanitizeText(input, filter) : sanitizeText(input);
  const length = graphemeCount(text);
  const issue: TextIssue | null =
    length < rules.min
      ? 'tooShort'
      : length > rules.max
        ? 'tooLong'
        : filter && looksBlocked(text, filter)
          ? 'notAllowed'
          : null;
  return { text, length, issue };
}
