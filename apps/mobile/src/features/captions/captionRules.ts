import { checkText, type TextCheck } from '../../text';

/**
 * Caption rules (prd-mvp US-12, SR-28): up to 80 grapheme clusters, emoji allowed, the shared text filter.
 * The default mirrors config/client.yaml `limits.caption_max_chars`; the served value wins when present. The
 * server's check at send time (T-30 filter) is authoritative; this only gives instant feedback.
 */
export const CAPTION_MAX_GRAPHEMES = 80;

/**
 * Hard cap on what the field accepts, in UTF-16 code units, so a huge paste never reaches the filter. Generous on
 * purpose: one emoji ZWJ sequence can take 11+ code units for a single grapheme.
 */
export const captionInputCap = (max: number) => max * 16;

/** An empty caption is fine (captions are optional). */
export function checkCaption(input: string, max = CAPTION_MAX_GRAPHEMES): TextCheck {
  return checkText(input, { min: 0, max });
}
