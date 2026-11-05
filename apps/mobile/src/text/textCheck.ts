import { graphemeCount } from './graphemes';
import { looksBlocked, sanitizeText } from './textFilter';

export type TextIssue = 'tooShort' | 'tooLong' | 'notAllowed';

export interface TextCheck {
  /** What will be sent and stored: the sanitized text (bidi controls and invisible characters removed). */
  text: string;
  /** Length in grapheme clusters of the sanitized text, the unit every server limit uses. */
  length: number;
  issue: TextIssue | null;
}

/** Client preview of the server's `TextFilter::check` for a field: sanitize, then length, then term lists. */
export function checkText(input: string, rules: { min: number; max: number }): TextCheck {
  const text = sanitizeText(input);
  const length = graphemeCount(text);
  const issue: TextIssue | null =
    length < rules.min
      ? 'tooShort'
      : length > rules.max
        ? 'tooLong'
        : looksBlocked(text)
          ? 'notAllowed'
          : null;
  return { text, length, issue };
}
