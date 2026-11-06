/**
 * The shared text filter's client hints as served by `GET /v1/config` (`text_filter`, from config/text_filter.yaml;
 * EM decision 2026-11-05, #213): the term lists are not bundled in the app. UX only: the server filter (T-30,
 * crates/platform/src/text.rs) checks again on submit and its answer is final.
 */
export interface TextFilterConfig {
  /** Combining marks kept per grapheme cluster (viramas exempt). */
  readonly maxMarksPerGrapheme: number;
  /** Grapheme clusters with more code points than this are dropped. */
  readonly maxCharsPerGrapheme: number;
  /** List name -> terms (`words`: whole folded words; `contains`: anywhere inside a folded word). */
  readonly lists: Readonly<
    Record<string, { readonly words: string[]; readonly contains: string[] }>
  >;
}

/** The only config shape this client understands. */
const SUPPORTED_VERSION = 1;
/** Bounds on served values: anything outside means a bad response, so the hints are skipped. */
const MAX_CAP = 64;
const MAX_TERMS = 5000;
const MAX_TERM_LENGTH = 64;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const cap = (value: unknown): number | undefined =>
  Number.isSafeInteger(value) && (value as number) >= 0 && (value as number) <= MAX_CAP
    ? (value as number)
    : undefined;

const isTerm = (term: unknown): term is string =>
  typeof term === 'string' && term.length > 0 && term.length <= MAX_TERM_LENGTH;

const terms = (value: unknown): string[] | undefined => {
  if (value === undefined) return [];
  return Array.isArray(value) && value.every(isTerm) ? (value as string[]) : undefined;
};

/**
 * Narrows the untrusted `text_filter` part of `/v1/config`. All or nothing: a malformed or unknown-version config
 * gives `undefined`, and the app then checks length and sanitizes only (no term hints).
 */
export function parseTextFilterConfig(body: unknown): TextFilterConfig | undefined {
  if (!isRecord(body) || body.version !== SUPPORTED_VERSION || !isRecord(body.lists)) {
    return undefined;
  }
  const maxMarksPerGrapheme = cap(body.max_marks_per_grapheme);
  const maxCharsPerGrapheme = cap(body.max_chars_per_grapheme);
  if (maxMarksPerGrapheme === undefined || !maxCharsPerGrapheme) return undefined;
  const lists: Record<string, { words: string[]; contains: string[] }> = {};
  let total = 0;
  for (const [name, list] of Object.entries(body.lists)) {
    if (!isRecord(list)) return undefined;
    const words = terms(list.words);
    const contains = terms(list.contains);
    if (!words || !contains) return undefined;
    total += words.length + contains.length;
    lists[name] = { words, contains };
  }
  if (total > MAX_TERMS) return undefined;
  return { maxMarksPerGrapheme, maxCharsPerGrapheme, lists };
}
