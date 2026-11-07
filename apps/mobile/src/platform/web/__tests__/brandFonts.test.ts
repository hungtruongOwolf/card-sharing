import appJson from '../../../../app.json';
import { BRAND_FACES } from '../../../ui/theme/fonts';
import { BRAND_FONT_FILES, loadBrandFonts } from '../brandFonts';

it('covers every brand face the type tokens use', () => {
  const faces = Object.values(BRAND_FACES).flatMap((byWeight) => Object.values(byWeight));
  expect(Object.keys(BRAND_FONT_FILES).sort()).toEqual([...new Set(faces)].sort());
});

it('loads the same files native builds embed (app.json expo-font plugin)', () => {
  const plugin = appJson.expo.plugins.find(
    (entry): entry is [string, { fonts: string[] }] =>
      Array.isArray(entry) && entry[0] === 'expo-font',
  );
  const embedded = (plugin?.[1].fonts ?? []).map((path) => path.replace(/^.*\/|\.ttf$/g, ''));
  expect(Object.keys(BRAND_FONT_FILES).sort()).toEqual(embedded.sort());
});

it('passes every face to the loader and never rejects', async () => {
  const load = jest.fn().mockRejectedValue(new Error('offline'));
  await expect(loadBrandFonts(load)).resolves.toBeUndefined();
  expect(Object.keys(load.mock.calls[0][0])).toHaveLength(Object.keys(BRAND_FONT_FILES).length);
});
