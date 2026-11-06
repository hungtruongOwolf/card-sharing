import { graphemes } from './graphemes';
import type { TextFilterConfig } from './textFilterConfig';

/**
 * Client mirror of the shared text filter (SR-28, threat-model A-21; crates/platform/src/text.rs) for instant
 * feedback while typing. UX only: the server sanitizes and checks again and its answer is final. Keep the rules
 * in step with the Rust module. The term lists and sanitizer caps come from the server (`GET /v1/config`
 * `text_filter`, #213); no term list is bundled in the app.
 */

/** Sanitizer caps used while the served config is unavailable; same values as config/text_filter.yaml. */
export const DEFAULT_SANITIZER_CAPS = { maxMarksPerGrapheme: 4, maxCharsPerGrapheme: 16 } as const;

type SanitizerCaps = Pick<TextFilterConfig, 'maxMarksPerGrapheme' | 'maxCharsPerGrapheme'>;

const ZWJ = '\u200D';
const ZWNJ = '\u200C';
const MARK = /\p{M}/u;
const LETTER_OR_DIGIT = /[\p{L}\p{N}]/u;
const LETTER = /\p{L}/u;
/** Rust `char::is_whitespace` (Unicode White_Space); JavaScript `\s` would also match U+FEFF. */
const WHITESPACE = /\p{White_Space}/u;

/**
 * Viramas link Indic conjuncts and are exempt from the mark cap. Rust reads the canonical combining class (9);
 * JavaScript has no API for it, so the common viramas are listed (Devanagari to Malayalam, Sinhala, Thai,
 * Myanmar, Khmer).
 */
const VIRAMAS = new Set(
  [
    0x094d, 0x09cd, 0x0a4d, 0x0acd, 0x0b4d, 0x0bcd, 0x0c4d, 0x0ccd, 0x0d4d, 0x0dca, 0x0e3a, 0x1039,
    0x103a, 0x17d2,
  ].map((cp) => String.fromCodePoint(cp)),
);

/** Same set as Rust `is_stripped`: controls, bidi controls, invisible formatting, tags, private use. */
function isStripped(char: string): boolean {
  const cp = char.codePointAt(0) ?? 0;
  return (
    cp < 0x20 ||
    (cp >= 0x7f && cp <= 0x9f) ||
    cp === 0x00ad ||
    cp === 0x034f ||
    cp === 0x061c ||
    cp === 0x17b4 ||
    cp === 0x17b5 ||
    (cp >= 0x180b && cp <= 0x180f) ||
    cp === 0x200b ||
    cp === 0x200e ||
    cp === 0x200f ||
    (cp >= 0x202a && cp <= 0x202e) ||
    (cp >= 0x2060 && cp <= 0x206f) ||
    cp === 0x2800 ||
    cp === 0x3164 ||
    cp === 0xffa0 ||
    (cp >= 0xfe00 && cp <= 0xfe0e) ||
    cp === 0xfeff ||
    (cp >= 0xfff0 && cp <= 0xfffb) ||
    (cp >= 0x1d173 && cp <= 0x1d17a) ||
    (cp >= 0xe0000 && cp <= 0xe007f) ||
    (cp >= 0xe0100 && cp <= 0xe01ef) ||
    (cp >= 0xe000 && cp <= 0xf8ff) ||
    cp >= 0xf0000 ||
    (cp & 0xfffe) === 0xfffe ||
    (cp >= 0xfdd0 && cp <= 0xfdef)
  );
}

const isHangulFiller = (char: string) => char === '\u115F' || char === '\u1160';

function cleanCluster(cluster: string, maxMarks: number): string {
  const chars = Array.from(cluster);
  let marks = 0;
  let out = '';
  chars.forEach((char, i) => {
    if (char === ZWJ) {
      const next = chars[i + 1];
      if (next !== undefined && next !== ZWJ && !MARK.test(next)) out += char;
    } else if (MARK.test(char) && !VIRAMAS.has(char)) {
      marks += 1;
      if (marks <= maxMarks) out += char;
    } else {
      out += char;
    }
  });
  return out;
}

function sanitizePass(input: string, caps: SanitizerCaps): string {
  // 1. NFC; drop invisible and control characters; any whitespace becomes a plain space.
  const visible = Array.from(input.normalize('NFC'), (char) =>
    WHITESPACE.test(char) ? ' ' : isStripped(char) ? '' : char,
  ).join('');
  // 2. Collapse whitespace runs and trim.
  const collapsed = visible.split(' ').filter(Boolean).join(' ');
  // 3. Per grapheme: cap marks, keep joining ZWJs only, drop over-long clusters.
  const clusters = graphemes(collapsed)
    .filter((cluster) => !Array.from(cluster).every(isHangulFiller))
    .map((cluster) => cleanCluster(cluster, caps.maxMarksPerGrapheme))
    .filter((cluster) => Array.from(cluster).length <= caps.maxCharsPerGrapheme);
  // 4. A non-joiner only stays between two non-ASCII letters.
  const chars = Array.from(clusters.join(''));
  const nonAsciiLetter = (char: string | undefined) =>
    char !== undefined && LETTER.test(char) && (char.codePointAt(0) ?? 0) > 0x7f;
  return chars
    .filter(
      (char, i) => char !== ZWNJ || (nonAsciiLetter(chars[i - 1]) && nonAsciiLetter(chars[i + 1])),
    )
    .join('')
    .normalize('NFC');
}

/** Single-line display form, as the server stores it. Idempotent. */
export function sanitizeText(input: string, caps: SanitizerCaps = DEFAULT_SANITIZER_CAPS): string {
  let current = sanitizePass(input, caps);
  for (let i = 0; i < 3; i++) {
    const next = sanitizePass(current, caps);
    if (next === current) break;
    current = next;
  }
  return current;
}

/** Look-alikes and leetspeak mapped to the Latin letters they imitate (Rust `substitute`). */
const SUBSTITUTIONS: Readonly<Record<string, string>> = Object.fromEntries(
  (
    [
      ['o', '0\u03BF\u043E\u00F8'],
      ['i', '1!|\u0131\u0456\u03B9\u0457'],
      ['e', '3\u0435\u03B5\u0454\u0451'],
      ['a', '4@\u0430\u03B1'],
      ['s', '5$\u0455'],
      ['t', '7+\u0442\u03C4'],
      ['b', '8\u0432\u03B2'],
      ['g', '9'],
      ['c', '\u0441\u03F2\u00A2'],
      ['p', '\u0440\u03C1'],
      ['y', '\u0443\u03B3'],
      ['x', '\u0445\u03C7'],
      ['k', '\u043A\u03BA'],
      ['m', '\u043C'],
      ['h', '\u043D\u03B7'],
      ['j', '\u0458'],
      ['d', '\u0501\u0111\u00F0'],
      ['v', '\u03BD'],
      ['u', '\u03C5'],
      ['w', '\u0448\u03C9'],
      ['l', '\u0142'],
      ['ss', '\u00DF'],
      ['ae', '\u00E6'],
      ['oe', '\u0153'],
    ] as const
  ).flatMap(([latin, lookAlikes]) => Array.from(lookAlikes, (char) => [char, latin])),
);

/** Lowercase folded words (Rust `fold`): compatibility forms, accents, case and look-alikes undone. */
export function foldWords(text: string): string[] {
  const words: string[] = [];
  let word = '';
  for (const char of text.normalize('NFKD')) {
    if (MARK.test(char)) continue;
    for (const lower of char.toLowerCase()) {
      const mapped = SUBSTITUTIONS[lower];
      if (mapped !== undefined) word += mapped;
      else if (LETTER_OR_DIGIT.test(lower)) word += lower;
      else if (word) {
        words.push(word);
        word = '';
      }
    }
  }
  if (word) words.push(word);
  return words;
}

/** The words plus every run of two or more one-letter words joined (`f.u.c.k`). */
function candidateWords(words: string[]): string[] {
  const out: string[] = [];
  let run = '';
  let runLength = 0;
  for (const word of words) {
    if (Array.from(word).length === 1) {
      run += word;
      runLength += 1;
    } else {
      if (runLength >= 2) out.push(run);
      run = '';
      runLength = 0;
    }
    out.push(word);
  }
  if (runLength >= 2) out.push(run);
  return out;
}

/** `fuuuck` -> `fuck`. */
const collapseRuns = (word: string) =>
  Array.from(word)
    .filter((char, i, all) => char !== all[i - 1])
    .join('');

interface FoldedList {
  words: Set<string>;
  contains: string[];
}

/** Folded lists per served config object, so typing does not refold every term on each keystroke. */
const foldedCache = new WeakMap<TextFilterConfig, FoldedList[]>();

function foldedLists(config: TextFilterConfig): FoldedList[] {
  const cached = foldedCache.get(config);
  if (cached) return cached;
  const fold = (term: string) => foldWords(term).join('');
  const lists = Object.values(config.lists).map((list) => ({
    words: new Set(list.words.map(fold).filter(Boolean)),
    contains: list.contains.map(fold).filter(Boolean),
  }));
  foldedCache.set(config, lists);
  return lists;
}

/** True when sanitized `text` would match one of the served term lists (Rust `blocked`). Never reveals which term. */
export function looksBlocked(text: string, config: TextFilterConfig): boolean {
  const lists = foldedLists(config);
  const plain = (term: string) => collapseRuns(term) === term;
  const candidates = candidateWords(foldWords(text)).map(
    (word) => [word, collapseRuns(word)] as const,
  );
  return lists.some((list) =>
    candidates.some(
      ([word, stretched]) =>
        list.words.has(word) ||
        (list.words.has(stretched) && plain(stretched)) ||
        list.contains.some(
          (term) => word.includes(term) || (plain(term) && stretched.includes(term)),
        ),
    ),
  );
}
