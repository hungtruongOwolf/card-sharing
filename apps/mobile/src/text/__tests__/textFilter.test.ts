import {
  checkText,
  fallbackGraphemes,
  graphemeCount,
  looksBlocked,
  parseTextFilterConfig,
  sanitizeText,
  type TextFilterConfig,
} from '..';

declare const __dirname: string;
const { readFileSync } = jest.requireActual('fs') as {
  readFileSync(file: string, encoding: 'utf8'): string;
};
const { join } = jest.requireActual('path') as { join(...parts: string[]): string };

/** Probes are built at runtime so the source holds no literal term (as in crates/platform/src/text.rs). */
const probe = (...letters: string[]) => letters.join('');
const TERM = probe('f', 'u', 'c', 'k');
const WORD_TERM = probe('s', 'h', 'i', 't');

/**
 * Minimal reader for the fixed shape of config/text_filter.yaml (no YAML dependency in the app). Tests only: the app
 * gets the same lists from `GET /v1/config` (#213), so the tests use the real lists the server would serve.
 */
function readFilterYaml() {
  const yaml = readFileSync(join(__dirname, '../../../../../config/text_filter.yaml'), 'utf8');
  const config: Record<string, number> = {};
  const lists: Record<string, Record<string, string[]>> = {};
  let list = '';
  let kind = '';
  for (const raw of yaml.split(/\r?\n/)) {
    const line = raw.replace(/#.*/, '').trimEnd();
    if (!line.trim()) continue;
    const indent = line.length - line.trimStart().length;
    const text = line.trim();
    if (indent === 0) {
      const [key, value] = text.split(':').map((part: string) => part.trim());
      if (key && value) config[key] = Number(value);
    } else if (indent === 2) {
      list = text.slice(0, -1);
      lists[list] = {};
    } else if (indent === 4) {
      kind = text.slice(0, -1);
      lists[list]![kind] = [];
    } else {
      lists[list]![kind]!.push(text.replace(/^- /, ''));
    }
  }
  return { config, lists };
}

/** The YAML as the server serves it in `/v1/config` `text_filter`, through the app's own parser. */
function servedFilter(): TextFilterConfig {
  const { config, lists } = readFilterYaml();
  const filter = parseTextFilterConfig({
    version: config.version,
    max_marks_per_grapheme: config.max_marks_per_grapheme,
    max_chars_per_grapheme: config.max_chars_per_grapheme,
    lists,
  });
  if (!filter) throw new Error('config/text_filter.yaml no longer parses as a served text filter');
  return filter;
}

const FILTER = servedFilter();

describe('served text filter config', () => {
  it('accepts the shipped YAML and rejects malformed or unknown-version configs', () => {
    expect(FILTER.maxMarksPerGrapheme).toBe(4);
    expect(Object.keys(FILTER.lists).length).toBeGreaterThan(0);
    const good = { version: 1, max_marks_per_grapheme: 4, max_chars_per_grapheme: 16, lists: {} };
    expect(parseTextFilterConfig(good)).toEqual({
      maxMarksPerGrapheme: 4,
      maxCharsPerGrapheme: 16,
      lists: {},
    });
    for (const bad of [
      null,
      'nope',
      { ...good, version: 2 },
      { ...good, max_chars_per_grapheme: 0 },
      { ...good, max_marks_per_grapheme: -1 },
      { ...good, lists: [] },
      { ...good, lists: { x: { words: [3] } } },
      { ...good, lists: { x: { words: [''] } } },
      { ...good, lists: { x: { contains: 'term' } } },
      { ...good, lists: { x: { words: Array(5001).fill('a') } } },
    ]) {
      expect(parseTextFilterConfig(bad)).toBeUndefined();
    }
  });
});

describe('sanitizeText (same rules as the server)', () => {
  it('normalizes to NFC and strips bidi controls and overrides', () => {
    expect(sanitizeText('Vie\u0323\u0302t')).toBe('Vi\u1EC7t');
    expect(sanitizeText('abc\u202Egpj.exe')).toBe('abcgpj.exe');
    for (const c of ['\u200E', '\u200F', '\u2066', '\u2067', '\u2068', '\u2069', '\u061C']) {
      expect(sanitizeText(`a${c}b`)).toBe('ab');
    }
  });

  it('removes invisible characters and collapses whitespace to one line', () => {
    for (const c of [
      '\u200B',
      '\u2060',
      '\uFEFF',
      '\u00AD',
      '\u034F',
      '\u3164',
      '\u2800',
      '\u{E0041}',
      '\uE000',
    ]) {
      expect(sanitizeText(`m${c}ai`)).toBe('mai');
    }
    expect(sanitizeText('  Mai\t\n\r  the\u3000cat\u00A0 ')).toBe('Mai the cat');
    expect(sanitizeText('a\u0007b\u0000c\u0085d')).toBe('abc d');
  });

  it('caps combining marks per grapheme (zalgo)', () => {
    const zalgo = `Z${'\u0336\u0301\u0310'.repeat(10)}a\u0352`;
    const clean = sanitizeText(zalgo);
    expect(graphemeCount(clean)).toBe(2);
    expect(Array.from(clean).filter((c) => /\p{M}/u.test(c)).length).toBeLessThanOrEqual(5);
  });

  it('keeps emoji sequences and joiners only where they join', () => {
    const family = '\u{1F468}\u200D\u{1F469}\u200D\u{1F467}';
    expect(sanitizeText(family)).toBe(family);
    expect(sanitizeText('\u{1F3F3}\uFE0F\u200D\u{1F308}')).toBe('\u{1F3F3}\uFE0F\u200D\u{1F308}');
    expect(sanitizeText('\u{1F44B}\u{1F3FD}')).toBe('\u{1F44B}\u{1F3FD}');
    expect(sanitizeText('m\u200Dai')).toBe('mai');
    expect(sanitizeText('\u0645\u06CC\u200C\u062E\u0648\u0627\u0647\u0645')).toBe(
      '\u0645\u06CC\u200C\u062E\u0648\u0627\u0647\u0645',
    );
    expect(sanitizeText('m\u200Cai')).toBe('mai');
  });

  it('drops over-long grapheme clusters and is idempotent', () => {
    const chain = Array(10).fill('\u{1F468}').join('\u200D');
    expect(sanitizeText(`ok${chain}`)).toBe('ok');
    for (const text of ['Mai the cat', 'Z\u0336\u0336\u0336\u0336\u0336a', ' a\u202E b ']) {
      expect(sanitizeText(sanitizeText(text))).toBe(sanitizeText(text));
    }
  });
});

describe('looksBlocked (hint only)', () => {
  it('matches whole-word terms as words and contains-terms inside words', () => {
    expect(looksBlocked(`oh ${WORD_TERM} it`, FILTER)).toBe(true);
    expect(looksBlocked(`super${TERM}cat`, FILTER)).toBe(true);
    expect(looksBlocked(TERM.split('').join('.'), FILTER)).toBe(true);
    for (const clean of [
      'Mai the cat',
      'Scunthorpe',
      'Cassandra',
      'cockatoo',
      'Jos\u00E9',
      '\u732B\u3061\u3083\u3093',
      'Zo\u00EB & Biscuit',
    ]) {
      expect(looksBlocked(clean, FILTER)).toBe(false);
    }
  });

  it('folds evasions before matching', () => {
    const [f, u, c, k] = TERM.split('');
    for (const evasion of [
      TERM.toUpperCase(),
      `${f}${u}${u}${u}${c}${k}`,
      `${f}${u}\u0441${k}`,
      `\uFF46${u}${c}${k}`,
      `${f}${u}\u0301${c}${k}`,
      `${f}\u200B${u}${c}${k}`,
    ]) {
      expect(looksBlocked(sanitizeText(evasion), FILTER)).toBe(true);
    }
    expect(looksBlocked(sanitizeText(probe('s', 'h', '1', 't')), FILTER)).toBe(true);
  });
});

describe('graphemes', () => {
  const samples = [
    'cat',
    'e\u0301',
    '\u{1F468}\u200D\u{1F469}\u200D\u{1F467}',
    '\u{1F44B}\u{1F3FD}',
    '\u{1F1FB}\u{1F1F3}\u{1F1EF}\u{1F1F5}',
    '\u{1F3F3}\uFE0F\u200D\u{1F308}',
    'Vi\u1EC7t Nam',
  ];

  it('count user-perceived characters, not code units', () => {
    expect(graphemeCount('\u{1F468}\u200D\u{1F469}\u200D\u{1F467}')).toBe(1);
    expect(graphemeCount('\u{1F1FB}\u{1F1F3}\u{1F1EF}\u{1F1F5}')).toBe(2);
    expect('\u{1F44B}\u{1F3FD}'.length).toBe(4);
    expect(graphemeCount('\u{1F44B}\u{1F3FD}')).toBe(1);
  });

  it('fallback clustering agrees with Intl.Segmenter on common text and emoji', () => {
    const segmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
    for (const sample of samples) {
      expect(fallbackGraphemes(sample)).toEqual(
        Array.from(segmenter.segment(sample), (s) => s.segment),
      );
    }
  });
});

describe('checkText', () => {
  it('reports the first issue on the sanitized text', () => {
    expect(checkText(' hi\u202E ', { min: 1, max: 80 })).toEqual({
      text: 'hi',
      length: 2,
      issue: null,
    });
    expect(checkText('\u200B', { min: 1, max: 80 }).issue).toBe('tooShort');
    expect(checkText('\u{1F431}'.repeat(81), { min: 0, max: 80 })).toMatchObject({
      length: 81,
      issue: 'tooLong',
    });
    expect(checkText(`my ${TERM}`, { min: 0, max: 80 }, FILTER).issue).toBe('notAllowed');
  });

  it('checks only length and sanitizing while the served config is unavailable', () => {
    expect(checkText(`my ${TERM}\u202E`, { min: 0, max: 80 })).toEqual({
      text: `my ${TERM}`,
      length: 3 + TERM.length,
      issue: null,
    });
    expect(checkText('\u{1F431}'.repeat(81), { min: 0, max: 80 }).issue).toBe('tooLong');
  });

  it('uses the served sanitizer caps', () => {
    const zalgo = `Z${'\u0336'.repeat(6)}`;
    const strict = { ...FILTER, maxMarksPerGrapheme: 1 };
    expect(Array.from(checkText(zalgo, { min: 0, max: 80 }, strict).text)).toHaveLength(2);
    expect(Array.from(checkText(zalgo, { min: 0, max: 80 }).text)).toHaveLength(5);
  });
});
