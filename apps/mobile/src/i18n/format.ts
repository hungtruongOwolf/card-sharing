/**
 * Minimal ICU MessageFormat formatter (no dependency).
 *
 * Supported syntax (the subset our copy needs):
 *   {name}                       argument (numbers are locale-formatted)
 *   {name, number}               locale-formatted number
 *   {name, plural, =0 {…} one {…} other {…}}   with `#` = the formatted number, optional `offset:n`
 *   {name, select, a {…} other {…}}
 *   ''  literal apostrophe;  '{…}'  quoted literal braces / #
 * Anything else (selectordinal, date, time, skeletons) is rejected at parse time so the catalog test fails early.
 *
 * Hermes does not guarantee `Intl.PluralRules`, so plural categories fall back to a built-in rule table.
 */

export type MessageValue = string | number;
export type MessageValues = Readonly<Record<string, MessageValue>>;

type Node =
  | { kind: 'text'; value: string }
  | { kind: 'arg'; name: string; format: 'none' | 'number' }
  | { kind: 'pound' }
  | { kind: 'plural'; name: string; offset: number; options: Map<string, Node[]> }
  | { kind: 'select'; name: string; options: Map<string, Node[]> };

export class MessageFormatError extends Error {
  override name = 'MessageFormatError';
}

const PLURAL_CATEGORIES = new Set(['zero', 'one', 'two', 'few', 'many', 'other']);

class Parser {
  private pos = 0;
  constructor(private readonly src: string) {}

  parse(): Node[] {
    const nodes = this.nodes(false, false);
    if (this.pos < this.src.length) this.fail('unexpected "}"');
    return nodes;
  }

  private fail(msg: string): never {
    throw new MessageFormatError(`${msg} at ${this.pos} in "${this.src}"`);
  }

  private nodes(nested: boolean, inPlural: boolean): Node[] {
    const out: Node[] = [];
    let text = '';
    const flush = () => {
      if (text) out.push({ kind: 'text', value: text });
      text = '';
    };
    while (this.pos < this.src.length) {
      const ch = this.src[this.pos] as string;
      if (ch === '}') {
        if (!nested) this.fail('unbalanced "}"');
        break;
      }
      if (ch === '{') {
        flush();
        out.push(this.argument());
        continue;
      }
      if (ch === '#' && inPlural) {
        flush();
        out.push({ kind: 'pound' });
        this.pos++;
        continue;
      }
      if (ch === "'") {
        text += this.apostrophe(inPlural);
        continue;
      }
      text += ch;
      this.pos++;
    }
    flush();
    return out;
  }

  /** ICU apostrophe rules: '' → ', and ' before a syntax char starts a quoted literal. */
  private apostrophe(inPlural: boolean): string {
    const next = this.src[this.pos + 1];
    if (next === "'") {
      this.pos += 2;
      return "'";
    }
    if (next === '{' || next === '}' || (inPlural && next === '#')) {
      const end = this.src.indexOf("'", this.pos + 1);
      if (end === -1) this.fail('unterminated quote');
      const literal = this.src.slice(this.pos + 1, end);
      this.pos = end + 1;
      return literal;
    }
    this.pos++;
    return "'";
  }

  private skipSpace() {
    while (/\s/.test(this.src[this.pos] ?? '')) this.pos++;
  }

  private word(): string {
    this.skipSpace();
    const m = /^[^\s{},]+/.exec(this.src.slice(this.pos));
    if (!m) this.fail('expected a name');
    this.pos += m[0].length;
    this.skipSpace();
    return m[0];
  }

  private expect(ch: string) {
    if (this.src[this.pos] !== ch) this.fail(`expected "${ch}"`);
    this.pos++;
  }

  private argument(): Node {
    this.expect('{');
    const name = this.word();
    if (!/^[A-Za-z_][\w]*$/.test(name)) this.fail(`invalid argument name "${name}"`);
    if (this.src[this.pos] === '}') {
      this.pos++;
      return { kind: 'arg', name, format: 'none' };
    }
    this.expect(',');
    const type = this.word();
    if (type === 'number') {
      this.expect('}');
      return { kind: 'arg', name, format: 'number' };
    }
    if (type !== 'plural' && type !== 'select') this.fail(`unsupported argument type "${type}"`);
    this.expect(',');
    let offset = 0;
    const options = new Map<string, Node[]>();
    for (;;) {
      this.skipSpace();
      if (this.src[this.pos] === '}') {
        this.pos++;
        break;
      }
      const key = this.word();
      if (type === 'plural' && key.startsWith('offset:')) {
        offset = Number(key.slice('offset:'.length));
        if (!Number.isFinite(offset)) this.fail('invalid offset');
        continue;
      }
      if (type === 'plural' && !PLURAL_CATEGORIES.has(key) && !/^=\d+$/.test(key)) {
        this.fail(`invalid plural selector "${key}"`);
      }
      if (options.has(key)) this.fail(`duplicate selector "${key}"`);
      this.expect('{');
      options.set(key, this.nodes(true, type === 'plural'));
      this.expect('}');
    }
    if (!options.has('other')) this.fail(`"${name}" needs an "other" option`);
    return type === 'plural'
      ? { kind: 'plural', name, offset, options }
      : { kind: 'select', name, options };
  }
}

const cache = new Map<string, Node[]>();

export function parseMessage(message: string): Node[] {
  let ast = cache.get(message);
  if (!ast) {
    ast = new Parser(message).parse();
    cache.set(message, ast);
  }
  return ast;
}

/** Argument names a message reads (used by the catalog test). */
export function messageArguments(message: string): string[] {
  const names = new Set<string>();
  const walk = (nodes: Node[]) => {
    for (const n of nodes) {
      if (n.kind === 'arg' || n.kind === 'plural' || n.kind === 'select') names.add(n.name);
      if (n.kind === 'plural' || n.kind === 'select') n.options.forEach(walk);
    }
  };
  walk(parseMessage(message));
  return [...names].sort();
}

type PluralCategory = 'zero' | 'one' | 'two' | 'few' | 'many' | 'other';

/** Fallback rules for engines without Intl.PluralRules (cardinal, integers and decimals). */
function fallbackPlural(locale: string, n: number): PluralCategory {
  const lang = locale.split('-')[0];
  const isInt = Number.isInteger(n);
  switch (lang) {
    case 'ja':
    case 'ko':
    case 'vi':
    case 'zh':
    case 'th':
    case 'id':
      return 'other';
    case 'fr':
    case 'pt':
      return n >= 0 && n < 2 ? 'one' : 'other';
    default:
      // English and most Germanic/Romance languages
      return isInt && n === 1 ? 'one' : 'other';
  }
}

const pluralRulesCache = new Map<string, Intl.PluralRules | null>();

export function pluralCategory(locale: string, n: number): PluralCategory {
  if (!pluralRulesCache.has(locale)) {
    const rules =
      typeof Intl !== 'undefined' && typeof Intl.PluralRules === 'function'
        ? new Intl.PluralRules(locale)
        : null;
    pluralRulesCache.set(locale, rules);
  }
  const rules = pluralRulesCache.get(locale);
  return rules ? (rules.select(n) as PluralCategory) : fallbackPlural(locale, n);
}

function formatNumber(locale: string, n: number): string {
  try {
    return new Intl.NumberFormat(locale).format(n);
  } catch {
    return String(n);
  }
}

function value(values: MessageValues, name: string): MessageValue {
  const v = values[name];
  if (v === undefined) throw new MessageFormatError(`missing value for "${name}"`);
  return v;
}

function numeric(values: MessageValues, name: string): number {
  const v = value(values, name);
  if (typeof v !== 'number') throw new MessageFormatError(`"${name}" must be a number`);
  return v;
}

function render(
  nodes: Node[],
  values: MessageValues,
  locale: string,
  pound: string | null,
): string {
  let out = '';
  for (const n of nodes) {
    switch (n.kind) {
      case 'text':
        out += n.value;
        break;
      case 'pound':
        out += pound ?? '#';
        break;
      case 'arg': {
        const v = n.format === 'number' ? numeric(values, n.name) : value(values, n.name);
        out += typeof v === 'number' ? formatNumber(locale, v) : v;
        break;
      }
      case 'plural': {
        const count = numeric(values, n.name);
        const adjusted = count - n.offset;
        const branch =
          n.options.get(`=${count}`) ??
          n.options.get(pluralCategory(locale, adjusted)) ??
          (n.options.get('other') as Node[]);
        out += render(branch, values, locale, formatNumber(locale, adjusted));
        break;
      }
      case 'select': {
        const key = String(value(values, n.name));
        out += render(
          n.options.get(key) ?? (n.options.get('other') as Node[]),
          values,
          locale,
          pound,
        );
        break;
      }
    }
  }
  return out;
}

/** Formats an ICU message. Throws MessageFormatError on bad syntax or missing values. */
export function formatMessage(message: string, values: MessageValues = {}, locale = 'en'): string {
  return render(parseMessage(message), values, locale, null);
}
