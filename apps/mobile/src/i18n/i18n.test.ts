import { formatMessage, messageArguments, MessageFormatError, pluralCategory } from './format';
import { t } from './index';
import { en, type MessageKey } from './messages/en';

describe('formatMessage', () => {
  it('substitutes arguments and formats numbers', () => {
    expect(formatMessage('Hi {name}', { name: 'Mai' })).toBe('Hi Mai');
    expect(formatMessage('{n, number} items', { n: 12345 })).toBe('12,345 items');
  });

  it('picks plural branches, exact matches first, with # as the number', () => {
    const msg = '{count, plural, =0 {none} one {# card} other {# cards}}';
    expect(formatMessage(msg, { count: 0 })).toBe('none');
    expect(formatMessage(msg, { count: 1 })).toBe('1 card');
    expect(formatMessage(msg, { count: 1200 })).toBe('1,200 cards');
  });

  it('supports offset and nested select', () => {
    const msg =
      '{who, select, me {You} other {{who}}} and {n, plural, offset:1 =1 {nobody else} one {# other} other {# others}}';
    expect(formatMessage(msg, { who: 'me', n: 1 })).toBe('You and nobody else');
    expect(formatMessage(msg, { who: 'Mai', n: 3 })).toBe('Mai and 2 others');
  });

  it('follows ICU apostrophe rules', () => {
    expect(formatMessage("It's {x}", { x: 'ok' })).toBe("It's ok");
    expect(formatMessage("Use '{braces}' and ''quotes''")).toBe("Use {braces} and 'quotes'");
  });

  it('rejects broken syntax and missing values', () => {
    expect(() => formatMessage('{count, plural, one {x}}', { count: 1 })).toThrow(
      MessageFormatError,
    );
    expect(() => formatMessage('{when, date}')).toThrow(MessageFormatError);
    expect(() => formatMessage('oops }')).toThrow(MessageFormatError);
    expect(() => formatMessage('Hi {name}')).toThrow(/missing value/);
    expect(() => formatMessage('{n, plural, other {#}}', { n: 'x' })).toThrow(/must be a number/);
  });

  it('falls back to built-in plural rules when Intl.PluralRules is missing (Hermes)', () => {
    const original = Intl.PluralRules;
    Object.defineProperty(Intl, 'PluralRules', {
      value: undefined,
      configurable: true,
      writable: true,
    });
    try {
      expect(pluralCategory('en-x-fallback', 1)).toBe('one');
      expect(pluralCategory('en-x-fallback', 2)).toBe('other');
      expect(pluralCategory('vi-x-fallback', 1)).toBe('other');
    } finally {
      Object.defineProperty(Intl, 'PluralRules', {
        value: original,
        configurable: true,
        writable: true,
      });
    }
  });
});

describe('English catalog', () => {
  const keys = Object.keys(en) as MessageKey[];

  it.each(keys)('%s parses and renders with sample values', (key) => {
    const values = Object.fromEntries(messageArguments(en[key]).map((name) => [name, 2]));
    expect(t(key, values).length).toBeGreaterThan(0);
  });

  it('uses the typographic apostrophe in copy', () => {
    for (const key of keys) expect(en[key]).not.toMatch(/[A-Za-z]'[A-Za-z]/);
  });

  it('renders plural copy without string concatenation', () => {
    expect(t('home.sent', { count: 0 })).toBe('Sent to nobody yet');
    expect(t('home.sent', { count: 1 })).toBe('Sent to 1 friend');
    expect(t('home.sent', { count: 3 })).toBe('Sent to 3 friends');
  });
});
