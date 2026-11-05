import { getLoadedFonts } from 'expo-font';

import { type as typeTokens, type TypeStyle } from '@card-sharing/tokens';

import { BRAND_FACES, fontStyleFor, resetFontCacheForTests } from '../theme/fonts';

// Node built-ins without @types/node in the app (the app itself never runs on Node).
declare const __dirname: string;
const fs = jest.requireActual('fs') as {
  existsSync(file: string): boolean;
  readFileSync(file: string, encoding: 'utf8'): string;
};
const path = jest.requireActual('path') as {
  resolve(...parts: string[]): string;
  join(...parts: string[]): string;
  dirname(file: string): string;
  basename(file: string, ext?: string): string;
};

const appRoot = path.resolve(__dirname, '../../..');
const mockedLoaded = getLoadedFonts as jest.MockedFunction<typeof getLoadedFonts>;

const allTypeStyles = (): TypeStyle[] =>
  Object.values(typeTokens).flatMap((sizes) => Object.values(sizes) as TypeStyle[]);

const embeddedFonts = (): string[] => {
  const app = JSON.parse(fs.readFileSync(path.join(appRoot, 'app.json'), 'utf8')) as {
    expo: { plugins: (string | [string, { fonts?: string[] }])[] };
  };
  const plugin = app.expo.plugins.find((p) => Array.isArray(p) && p[0] === 'expo-font');
  return Array.isArray(plugin) ? (plugin[1].fonts ?? []) : [];
};

afterEach(() => {
  resetFontCacheForTests();
  mockedLoaded.mockRestore?.();
});

describe('fontStyleFor', () => {
  it('uses the brand face without a fontWeight once the OS reports it loaded', () => {
    expect(fontStyleFor('display', '700')).toEqual({ fontFamily: 'BricolageGrotesque-Bold' });
    expect(fontStyleFor('body', '400')).toEqual({ fontFamily: 'DMSans-Regular' });
  });

  it('falls back to the platform font with the token weight until the fonts are loaded', () => {
    mockedLoaded.mockReturnValueOnce([]);
    expect(fontStyleFor('display', '700')).toEqual({ fontWeight: '700' });
  });

  it('falls back when the native module is missing', () => {
    mockedLoaded.mockImplementationOnce(() => {
      throw new Error('native module not found');
    });
    expect(fontStyleFor('body', '600')).toEqual({ fontWeight: '600' });
  });

  it('falls back for a weight that is not bundled', () => {
    expect(fontStyleFor('hand', '700')).toEqual({ fontWeight: '700' });
  });
});

describe('bundled faces stay in sync (change-impact guard)', () => {
  it('every type token has a bundled face', () => {
    for (const style of allTypeStyles()) {
      expect(BRAND_FACES[style.family][style.weight]).toBeDefined();
    }
  });

  it('every face is embedded by the expo-font plugin from a file named after it, with its OFL.txt', () => {
    const embedded = embeddedFonts();
    const faces = Object.values(BRAND_FACES).flatMap((w) => Object.values(w));
    expect(embedded.map((f) => path.basename(f, '.ttf')).sort()).toEqual([...faces].sort());
    for (const file of embedded) {
      const abs = path.join(appRoot, file);
      expect(fs.existsSync(abs)).toBe(true);
      expect(fs.existsSync(path.join(path.dirname(abs), 'OFL.txt'))).toBe(true);
    }
  });
});
