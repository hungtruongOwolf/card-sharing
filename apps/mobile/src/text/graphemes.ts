/**
 * Grapheme clusters: what a person sees as one character, and what the server counts for every length limit
 * (crates/platform/src/text.rs, `unicode-segmentation` extended clusters).
 */

const MARK = /\p{M}/u;
const ZWJ = '\u200D';
const isEmojiModifier = (cp: number) => cp >= 0x1f3fb && cp <= 0x1f3ff;
const isRegionalIndicator = (cp: number) => cp >= 0x1f1e6 && cp <= 0x1f1ff;
const isTag = (cp: number) => cp >= 0xe0020 && cp <= 0xe007f;
const isVariationSelector = (cp: number) =>
  (cp >= 0xfe00 && cp <= 0xfe0f) || (cp >= 0xe0100 && cp <= 0xe01ef);

/**
 * Approximate extended grapheme clustering for engines without `Intl.Segmenter`: combining marks, variation
 * selectors, emoji modifiers, tag characters and ZWJ sequences extend a cluster; regional indicators pair up.
 * Hangul jamo and Indic conjuncts may split differently from the full UAX #29 rules (a UX count only).
 */
export function fallbackGraphemes(text: string): string[] {
  const clusters: string[] = [];
  let joinNext = false;
  let regionalOpen = false;
  for (const char of text) {
    const cp = char.codePointAt(0) ?? 0;
    const last = clusters.length - 1;
    const extends_ =
      last >= 0 &&
      (joinNext ||
        MARK.test(char) ||
        char === ZWJ ||
        isVariationSelector(cp) ||
        isEmojiModifier(cp) ||
        isTag(cp) ||
        (isRegionalIndicator(cp) && regionalOpen));
    if (extends_) {
      clusters[last] += char;
      regionalOpen = false;
    } else {
      clusters.push(char);
      regionalOpen = isRegionalIndicator(cp);
    }
    joinNext = char === ZWJ;
  }
  return clusters;
}

type SegmenterLike = { segment(text: string): Iterable<{ segment: string }> };

function nativeSegmenter(): SegmenterLike | null {
  const intl = Intl as unknown as {
    Segmenter?: new (locale?: string, options?: { granularity: 'grapheme' }) => SegmenterLike;
  };
  return intl.Segmenter ? new intl.Segmenter(undefined, { granularity: 'grapheme' }) : null;
}

let segmenter: SegmenterLike | null | undefined;

/** The grapheme clusters of `text`, with `Intl.Segmenter` where the engine has it. */
export function graphemes(text: string): string[] {
  if (segmenter === undefined) segmenter = nativeSegmenter();
  if (!segmenter) return fallbackGraphemes(text);
  return Array.from(segmenter.segment(text), (part) => part.segment);
}

export const graphemeCount = (text: string): number => graphemes(text).length;
